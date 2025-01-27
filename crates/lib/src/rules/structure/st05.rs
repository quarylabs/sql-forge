use std::iter::zip;
use std::ops::{Index, IndexMut};

use ahash::{AHashMap, AHashSet};
use itertools::{enumerate, Itertools};
use smol_str::{format_smolstr, SmolStr, StrExt, ToSmolStr};
use sqruff_lib_core::dialects::base::Dialect;
use sqruff_lib_core::dialects::common::AliasInfo;
use sqruff_lib_core::dialects::init::DialectKind;
use sqruff_lib_core::dialects::syntax::{SyntaxKind, SyntaxSet};
use sqruff_lib_core::lint_fix::LintFix;
use sqruff_lib_core::linter::compute_anchor_edit_info;
use sqruff_lib_core::parser::segments::base::{ErasedSegment, SegmentBuilder, Tables};
use sqruff_lib_core::parser::segments::object_reference::ObjectReferenceLevel;
use sqruff_lib_core::utils::analysis::query::{Query, Selectable};
use sqruff_lib_core::utils::analysis::select::get_select_statement_info;
use sqruff_lib_core::utils::functional::segments::Segments;

use crate::core::config::Value;
use crate::core::rules::base::{Erased, ErasedRule, LintResult, Rule, RuleGroups};
use crate::core::rules::context::RuleContext;
use crate::core::rules::crawlers::{Crawler, SegmentSeekerCrawler};
use crate::utils::functional::context::FunctionalContext;

const SELECT_TYPES: SyntaxSet = SyntaxSet::new(&[
    SyntaxKind::WithCompoundStatement,
    SyntaxKind::SetExpression,
    SyntaxKind::SelectStatement,
]);

static CONFIG_MAPPING: phf::Map<&str, SyntaxSet> = phf::phf_map! {
    "join" => SyntaxSet::single(SyntaxKind::JoinClause),
    "from" => SyntaxSet::single(SyntaxKind::FromExpressionElement),
    "both" => SyntaxSet::new(&[SyntaxKind::JoinClause, SyntaxKind::FromExpressionElement])
};

#[allow(dead_code)]
struct NestedSubQuerySummary<'a> {
    query: Query<'a, ()>,
    selectable: Selectable<'a>,
    table_alias: AliasInfo,
    select_source_names: AHashSet<SmolStr>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RuleST05 {
    forbid_subquery_in: String,
}

impl Rule for RuleST05 {
    fn load_from_config(&self, config: &AHashMap<String, Value>) -> Result<ErasedRule, String> {
        Ok(RuleST05 {
            forbid_subquery_in: config["forbid_subquery_in"].as_string().unwrap().into(),
        }
        .erased())
    }

    fn name(&self) -> &'static str {
        "structure.subquery"
    }

    fn description(&self) -> &'static str {
        "Join/From clauses should not contain subqueries. Use CTEs instead."
    }

    fn long_description(&self) -> &'static str {
        r"
## Anti-pattern

Join is a sub query in a `FROM` clause. This can make the query harder to read and maintain.

```sql
select
    a.x, a.y, b.z
from a
join (
    select x, z from b
) using(x)
```

## Best practice

Use a Common Table Expression (CTE) to define the subquery and then join it to the main query.

```sql
with c as (
    select x, z from b
)
select
    a.x, a.y, c.z
from a
join c using(x)
```
"
    }

    fn groups(&self) -> &'static [RuleGroups] {
        &[RuleGroups::All, RuleGroups::Structure]
    }

    fn eval(&self, context: &RuleContext) -> Vec<LintResult> {
        let functional_context = FunctionalContext::new(context);
        let segment = functional_context.segment();
        let parent_stack = functional_context.parent_stack();

        let is_select = segment.all(Some(|it: &ErasedSegment| {
            SELECT_TYPES.contains(it.get_type())
        }));
        let is_select_child = parent_stack.any(Some(|it: &ErasedSegment| {
            SELECT_TYPES.contains(it.get_type())
        }));

        if !is_select || is_select_child {
            return Vec::new();
        }

        let query: Query<'_, ()> = Query::from_segment(&context.segment, context.dialect, None);
        let mut ctes = CTEBuilder::default();

        for cte in query.inner.borrow().ctes.values() {
            ctes.insert_cte(cte.inner.borrow().cte_definition_segment.clone().unwrap());
        }

        let is_with = segment.all(Some(|it: &ErasedSegment| {
            it.is_type(SyntaxKind::WithCompoundStatement)
        }));
        let is_recursive = is_with
            && !segment
                .children(Some(|it: &ErasedSegment| it.is_keyword("recursive")))
                .is_empty();

        let case_preference = get_case_preference(&segment);

        let clone_map = SegmentCloneMap::new(
            segment.first().unwrap().clone(),
            segment.first().unwrap().deep_clone(),
        );

        let results = self.lint_query(
            context.tables,
            context.dialect,
            query,
            &mut ctes,
            case_preference,
            &clone_map,
        );

        let mut lint_results = Vec::with_capacity(results.len());
        let mut is_fixable = true;

        let mut subquery_parent = None;

        let mut local_fixes = Vec::new();
        let mut q = Vec::new();

        for result in results {
            let (lint_result, from_expression, alias_name, subquery_parent_slot) = result;
            subquery_parent = Some(subquery_parent_slot.clone());
            let this_seg_clone = clone_map[&from_expression].clone();
            let new_table_ref = create_table_ref(context.tables, &alias_name, context.dialect);

            local_fixes.push(LintFix::replace(
                this_seg_clone.clone(),
                vec![SegmentBuilder::node(
                    context.tables.next_id(),
                    this_seg_clone.get_type(),
                    context.dialect.name,
                    vec![new_table_ref],
                )
                .finish()],
                None,
            ));

            q.push(subquery_parent_slot);

            let bracketed_ctas = parent_stack
                .base
                .iter()
                .rev()
                .take(2)
                .map(|it| it.get_type())
                .eq([SyntaxKind::CreateTableStatement, SyntaxKind::Bracketed]);

            if bracketed_ctas || ctes.has_duplicate_aliases() || is_recursive {
                is_fixable = false;
            }

            lint_results.push(lint_result);
        }

        if !is_fixable {
            return lint_results;
        }

        let mut fixes = compute_anchor_edit_info(local_fixes.into_iter());
        let (new_root, _, _, _) = clone_map.root.apply_fixes(&mut fixes);

        let clone_map = SegmentCloneMap::new(segment.first().unwrap().clone(), new_root.clone());
        for subquery_parent_slot in q {
            ctes.replace_with_clone(subquery_parent_slot, &clone_map);
        }

        let _segment = Segments::new(new_root, None);
        let output_select = if is_with {
            _segment.children(Some(|it: &ErasedSegment| {
                matches!(
                    it.get_type(),
                    SyntaxKind::SetExpression | SyntaxKind::SelectStatement
                )
            }))
        } else {
            _segment.clone()
        };

        for result in &mut lint_results {
            let subquery_parent = subquery_parent.clone().unwrap();
            let output_select_clone = output_select[0].clone();

            let mut fixes = ctes.ensure_space_after_from(
                context.tables,
                output_select[0].clone(),
                &output_select_clone,
                subquery_parent,
            );

            let new_select = ctes.compose_select(
                context.tables,
                context.dialect.name,
                output_select_clone.clone(),
                case_preference,
            );

            result.fixes = vec![LintFix::replace(
                segment.first().unwrap().clone(),
                vec![new_select],
                None,
            )];

            result.fixes.append(&mut fixes);
        }

        lint_results
    }

    fn is_fix_compatible(&self) -> bool {
        true
    }

    fn crawl_behaviour(&self) -> Crawler {
        SegmentSeekerCrawler::new(SELECT_TYPES).into()
    }
}

impl RuleST05 {
    fn lint_query<'a>(
        &self,
        tables: &Tables,
        dialect: &'a Dialect,
        query: Query<'a, ()>,
        ctes: &mut CTEBuilder,
        case_preference: Case,
        segment_clone_map: &SegmentCloneMap,
    ) -> Vec<(LintResult, ErasedSegment, SmolStr, ErasedSegment)> {
        let mut acc = Vec::new();

        for nsq in self.nested_subqueries(query, dialect) {
            let (alias_name, _) = ctes.create_cte_alias(Some(&nsq.table_alias));
            let anchor = nsq
                .table_alias
                .from_expression_element
                .segments()
                .first()
                .cloned()
                .unwrap();

            let new_cte = create_cte_seg(
                tables,
                alias_name.clone(),
                segment_clone_map[&anchor].clone(),
                case_preference,
                dialect,
            );

            ctes.insert_cte(new_cte);

            if nsq.query.inner.borrow().selectables.len() != 1 {
                continue;
            }

            let select = nsq.query.inner.borrow().selectables[0].clone().selectable;
            let anchor = anchor.recursive_crawl(
                const {
                    &SyntaxSet::new(&[
                        SyntaxKind::Keyword,
                        SyntaxKind::Symbol,
                        SyntaxKind::StartBracket,
                        SyntaxKind::EndBracket,
                    ])
                },
                true,
                &SyntaxSet::EMPTY,
                true,
            )[0]
            .clone();

            let res = LintResult::new(
                anchor.into(),
                Vec::new(),
                format!(
                    "{} clauses should not contain subqueries. Use CTEs instead",
                    select.get_type().as_str()
                )
                .into(),
                None,
            );

            acc.push((
                res,
                nsq.table_alias.from_expression_element,
                alias_name.clone(),
                nsq.query.inner.borrow().selectables[0].clone().selectable,
            ));
        }

        acc
    }

    fn nested_subqueries<'a>(
        &self,
        query: Query<'a, ()>,
        dialect: &'a Dialect,
    ) -> Vec<NestedSubQuerySummary<'a>> {
        let mut acc = Vec::new();

        let parent_types = &CONFIG_MAPPING[&self.forbid_subquery_in];
        let mut queries = vec![query.clone()];
        queries.extend(query.inner.borrow().ctes.values().cloned());

        for (i, q) in enumerate(queries) {
            for selectable in &q.inner.borrow().selectables {
                let Some(select_info) = selectable.select_info() else {
                    continue;
                };

                let mut select_source_names = AHashSet::new();
                for table_alias in select_info.table_aliases {
                    if !table_alias.ref_str.is_empty() {
                        select_source_names.insert(table_alias.ref_str.clone());
                    }

                    if let Some(object_reference) = &table_alias.object_reference {
                        select_source_names.insert(object_reference.raw().to_smolstr());
                    }

                    let Some(query) =
                        Query::<()>::from_root(&table_alias.from_expression_element, dialect)
                    else {
                        continue;
                    };

                    let path_to = selectable
                        .selectable
                        .path_to(&table_alias.from_expression_element);

                    if !(parent_types.contains(table_alias.from_expression_element.get_type())
                        || path_to
                            .iter()
                            .any(|ps| parent_types.contains(ps.segment.get_type())))
                    {
                        continue;
                    }

                    if is_correlated_subquery(
                        Segments::new(
                            query
                                .inner
                                .borrow()
                                .selectables
                                .first()
                                .unwrap()
                                .selectable
                                .clone(),
                            None,
                        ),
                        &select_source_names,
                        dialect,
                    ) {
                        continue;
                    }

                    acc.push(NestedSubQuerySummary {
                        query: q.clone(),
                        selectable: selectable.clone(),
                        table_alias: table_alias.clone(),
                        select_source_names: select_source_names.clone(),
                    });

                    if i > 0 {
                        acc.append(&mut self.nested_subqueries(query.clone(), dialect));
                    }
                }
            }
        }

        acc
    }
}

fn get_first_select_statement_descendant(segment: &ErasedSegment) -> Option<ErasedSegment> {
    segment
        .recursive_crawl(
            const { &SyntaxSet::single(SyntaxKind::SelectStatement) },
            false,
            &SyntaxSet::EMPTY,
            true,
        )
        .into_iter()
        .next()
}

fn is_correlated_subquery(
    nested_select: Segments,
    select_source_names: &AHashSet<SmolStr>,
    dialect: &Dialect,
) -> bool {
    let Some(select_statement) =
        get_first_select_statement_descendant(nested_select.first().unwrap())
    else {
        return false;
    };

    let nested_select_info = get_select_statement_info(&select_statement, dialect.into(), true);
    if let Some(nested_select_info) = nested_select_info {
        for r in nested_select_info.reference_buffer {
            for tr in r.extract_possible_references(ObjectReferenceLevel::Table, dialect.name) {
                if select_source_names.contains(&tr.part.to_smolstr()) {
                    return true;
                }
            }
        }
    }

    false
}

#[derive(Default)]
struct CTEBuilder {
    ctes: Vec<ErasedSegment>,
    name_idx: usize,
}

impl CTEBuilder {
    fn get_cte_segments(&self, tables: &Tables) -> Vec<ErasedSegment> {
        let mut cte_segments = Vec::new();
        let mut ctes = self.ctes.iter().peekable();

        while let Some(cte) = ctes.next() {
            cte_segments.push(cte.clone());

            if ctes.peek().is_some() {
                cte_segments.extend([
                    SegmentBuilder::comma(tables.next_id()),
                    SegmentBuilder::newline(tables.next_id(), "\n"),
                ]);
            }
        }

        cte_segments
    }

    fn compose_select(
        &self,
        tables: &Tables,
        dialect: DialectKind,
        output_select_clone: ErasedSegment,
        case_preference: Case,
    ) -> ErasedSegment {
        let mut segments = vec![
            segmentify(tables, "WITH", case_preference),
            SegmentBuilder::whitespace(tables.next_id(), " "),
        ];
        segments.extend(self.get_cte_segments(tables));
        segments.push(SegmentBuilder::newline(tables.next_id(), "\n"));
        segments.push(output_select_clone);

        SegmentBuilder::node(
            tables.next_id(),
            SyntaxKind::WithCompoundStatement,
            dialect,
            segments,
        )
        .finish()
    }
}

impl CTEBuilder {
    fn ensure_space_after_from(
        &self,
        tables: &Tables,
        output_select: ErasedSegment,
        output_select_clone: &ErasedSegment,
        subquery_parent: ErasedSegment,
    ) -> Vec<LintFix> {
        let mut fixes = Vec::new();

        if subquery_parent.is(&output_select) {
            let (missing_space_after_from, _from_clause, _from_clause_children, _from_segment) =
                Self::missing_space_after_from(output_select_clone.clone());

            if missing_space_after_from {
                todo!()
            }
        } else {
            let (missing_space_after_from, _from_clause, _from_clause_children, from_segment) =
                Self::missing_space_after_from(subquery_parent);

            if missing_space_after_from {
                fixes.push(LintFix::create_after(
                    from_segment.unwrap().base[0].clone(),
                    vec![SegmentBuilder::whitespace(tables.next_id(), " ")],
                    None,
                ))
            }
        }

        fixes
    }

    fn missing_space_after_from(
        segment: ErasedSegment,
    ) -> (
        bool,
        Option<ErasedSegment>,
        Option<ErasedSegment>,
        Option<Segments>,
    ) {
        let mut missing_space_after_from = false;
        let from_clause_children = None;
        let mut from_segment = None;
        let from_clause = segment.child(const { &SyntaxSet::single(SyntaxKind::FromClause) });

        if let Some(from_clause) = &from_clause {
            let from_clause_children = Segments::from_vec(from_clause.segments().to_vec(), None);
            from_segment = from_clause_children
                .find_first(Some(|it: &ErasedSegment| it.is_keyword("FROM")))
                .into();
            if !from_segment.as_ref().unwrap().is_empty()
                && from_clause_children
                    .select::<fn(&ErasedSegment) -> bool>(
                        None,
                        Some(|it| it.is_whitespace()),
                        Some(&from_segment.as_ref().unwrap().base[0]),
                        None,
                    )
                    .is_empty()
            {
                missing_space_after_from = true;
            }
        }

        (
            missing_space_after_from,
            from_clause,
            from_clause_children,
            from_segment,
        )
    }
}

impl CTEBuilder {
    pub(crate) fn replace_with_clone(
        &mut self,
        segment: ErasedSegment,
        clone_map: &SegmentCloneMap,
    ) {
        for (idx, cte) in enumerate(&self.ctes) {
            if cte.recursive_crawl_all(false).any(|seg| segment.is(&seg)) {
                self.ctes[idx] = clone_map[&self.ctes[idx]].clone();
                return;
            }
        }
    }
}

impl CTEBuilder {
    fn list_used_names(&self) -> Vec<SmolStr> {
        let mut used_names = Vec::new();

        for cte in &self.ctes {
            let id_seg = cte
                .child(
                    const {
                        &SyntaxSet::new(&[
                            SyntaxKind::Identifier,
                            SyntaxKind::NakedIdentifier,
                            SyntaxKind::QuotedIdentifier,
                        ])
                    },
                )
                .unwrap();

            let cte_name = if id_seg.is_type(SyntaxKind::QuotedIdentifier) {
                let raw = id_seg.raw();
                raw[1..raw.len() - 1].to_smolstr()
            } else {
                id_seg.raw().to_smolstr()
            };

            used_names.push(cte_name);
        }

        used_names
    }

    fn has_duplicate_aliases(&self) -> bool {
        let used_names = self.list_used_names();
        !used_names.into_iter().all_unique()
    }

    fn create_cte_alias(&mut self, alias: Option<&AliasInfo>) -> (SmolStr, bool) {
        if let Some(alias) = alias.filter(|alias| alias.aliased && !alias.ref_str.is_empty()) {
            return (alias.ref_str.clone(), false);
        }

        self.name_idx += 1;
        let name = format_smolstr!("prep_{}", self.name_idx);
        if self.list_used_names().iter().contains(&name) {
            return self.create_cte_alias(None);
        }

        (name, true)
    }

    fn insert_cte(&mut self, cte: ErasedSegment) {
        let inbound_subquery = Segments::new(cte.clone(), None)
            .children(None)
            .find_first(Some(|it: &ErasedSegment| {
                it.get_position_marker().is_some()
            }));
        let insert_position = self
            .ctes
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, it)| {
                is_child(
                    Segments::new(
                        Segments::new(it.clone(), None)
                            .children(None)
                            .last()
                            .cloned()
                            .unwrap(),
                        None,
                    ),
                    inbound_subquery.clone(),
                )
            })
            .map(|(i, _)| i)
            .next()
            .unwrap_or(self.ctes.len());

        self.ctes.insert(insert_position, cte);
    }
}

fn is_child(maybe_parent: Segments, maybe_child: Segments) -> bool {
    let child_markers = maybe_child[0].get_position_marker().unwrap();
    let parent_pos = maybe_parent[0].get_position_marker().unwrap();

    if child_markers < &parent_pos.start_point_marker() {
        return false;
    }

    if child_markers > &parent_pos.end_point_marker() {
        return false;
    }

    true
}

#[derive(Eq, PartialEq, Copy, Clone)]
enum Case {
    Lower,
    Upper,
}

fn get_case_preference(root_select: &Segments) -> Case {
    let root_segment = root_select.first().expect("Root SELECT not found");
    let first_keyword = root_segment.recursive_crawl(
        const { &SyntaxSet::single(SyntaxKind::Keyword) },
        false,
        &SyntaxSet::EMPTY,
        true,
    )[0]
    .clone();

    if first_keyword.raw().chars().all(char::is_lowercase) {
        Case::Lower
    } else {
        Case::Upper
    }
}

fn segmentify(tables: &Tables, input_el: &str, casing: Case) -> ErasedSegment {
    let mut input_el = input_el.to_lowercase_smolstr();
    if casing == Case::Upper {
        input_el = input_el.to_uppercase_smolstr();
    }
    SegmentBuilder::keyword(tables.next_id(), &input_el)
}

fn create_cte_seg(
    tables: &Tables,
    alias_name: SmolStr,
    subquery: ErasedSegment,
    case_preference: Case,
    dialect: &Dialect,
) -> ErasedSegment {
    SegmentBuilder::node(
        tables.next_id(),
        SyntaxKind::CommonTableExpression,
        dialect.name,
        vec![
            SegmentBuilder::token(tables.next_id(), &alias_name, SyntaxKind::NakedIdentifier)
                .finish(),
            SegmentBuilder::whitespace(tables.next_id(), " "),
            segmentify(tables, "AS", case_preference),
            SegmentBuilder::whitespace(tables.next_id(), " "),
            subquery,
        ],
    )
    .finish()
}

fn create_table_ref(tables: &Tables, table_name: &str, dialect: &Dialect) -> ErasedSegment {
    SegmentBuilder::node(
        tables.next_id(),
        SyntaxKind::TableExpression,
        dialect.name,
        vec![SegmentBuilder::node(
            tables.next_id(),
            SyntaxKind::TableReference,
            dialect.name,
            vec![
                SegmentBuilder::token(tables.next_id(), table_name, SyntaxKind::NakedIdentifier)
                    .finish(),
            ],
        )
        .finish()],
    )
    .finish()
}

pub(crate) struct SegmentCloneMap {
    root: ErasedSegment,
    segment_map: AHashMap<usize, ErasedSegment>,
}

impl Index<&ErasedSegment> for SegmentCloneMap {
    type Output = ErasedSegment;

    fn index(&self, index: &ErasedSegment) -> &Self::Output {
        &self.segment_map[&index.addr()]
    }
}

impl IndexMut<&ErasedSegment> for SegmentCloneMap {
    fn index_mut(&mut self, index: &ErasedSegment) -> &mut Self::Output {
        self.segment_map.get_mut(&index.addr()).unwrap()
    }
}

impl SegmentCloneMap {
    fn new(segment: ErasedSegment, segment_copy: ErasedSegment) -> Self {
        let mut segment_map = AHashMap::new();

        for (old_segment, new_segment) in zip(
            segment.recursive_crawl_all(false),
            segment_copy.recursive_crawl_all(false),
        ) {
            segment_map.insert(old_segment.addr(), new_segment);
        }

        Self {
            root: segment_copy,
            segment_map,
        }
    }
}

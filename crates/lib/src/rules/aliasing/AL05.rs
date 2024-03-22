use crate::core::dialects::base::Dialect;
use crate::core::dialects::common::AliasInfo;
use crate::core::rules::base::{LintFix, LintResult, Rule};
use crate::core::rules::context::RuleContext;
use crate::core::rules::crawlers::{Crawler, SegmentSeekerCrawler};
use crate::utils::analysis::query::Query;
use crate::utils::analysis::select::get_select_statement_info;
use crate::utils::functional::segments::Segments;

#[derive(Default)]
struct AL05Query {
    aliases: Vec<AliasInfo>,
    tbl_refs: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct RuleAL05 {}

impl Rule for RuleAL05 {
    fn crawl_behaviour(&self) -> Crawler {
        SegmentSeekerCrawler::new(["select_statement"].into()).into()
    }

    fn eval(&self, context: RuleContext) -> Vec<LintResult> {
        let mut violations = Vec::new();
        let select_info =
            get_select_statement_info(&context.segment, (&context.dialect).into(), true);

        let Some(select_info) = select_info else {
            return Vec::new();
        };

        if select_info.table_aliases.is_empty() {
            return Vec::new();
        }

        let mut query = Query::from_segment(&context.segment, &context.dialect, None);
        self.analyze_table_aliases(&mut query, &context.dialect);

        for alias in query.payload.aliases {
            // if alias.from_expression_element and self._is_alias_required(
            //     alias.from_expression_element, context.dialect.name
            // ):
            //     continue
            if alias.aliased && !query.payload.tbl_refs.contains(&alias.ref_str) {
                let violation = self.report_unused_alias(alias);
                violations.push(violation);
            }
        }

        violations
    }
}

impl RuleAL05 {
    fn analyze_table_aliases(&self, query: &mut Query<AL05Query>, dialect: &Dialect) {
        for selectable in &query.selectables {
            if let Some(select_info) = selectable.select_info() {
                query.payload.aliases.extend(select_info.table_aliases);

                for _r in select_info.reference_buffer {
                    unimplemented!();
                }
            }
        }

        for child in query.children_mut() {
            self.analyze_table_aliases(child, dialect);
        }
    }

    fn report_unused_alias(&self, alias: AliasInfo) -> LintResult {
        let mut fixes = vec![LintFix::delete(alias.alias_expression.clone().unwrap())];
        let to_delete = Segments::from_vec(alias.from_expression_element.segments().to_vec(), None)
            .reversed()
            .select(
                None,
                Some(|it| it.is_whitespace() || it.is_meta()),
                alias.alias_expression.as_ref().map(|it| it.as_ref()).unwrap().into(),
                None,
            );

        fixes.extend(to_delete.into_iter().map(|it| LintFix::delete(it)));

        LintResult::new(alias.segment, fixes, None, None, None)
    }
}

#[cfg(test)]
mod tests {
    use crate::api::simple::{fix, lint};
    use crate::core::rules::base::{Erased, ErasedRule};
    use crate::rules::aliasing::AL05::RuleAL05;

    fn rules() -> Vec<ErasedRule> {
        vec![RuleAL05::default().erased()]
    }

    #[test]
    fn test_fail_table_alias_not_referenced_1() {
        let fail_str = "SELECT * FROM my_tbl AS foo";
        let fix_str = "SELECT * FROM my_tbl";

        let result = fix(fail_str.into(), rules());
        assert_eq!(fix_str, result);
    }
}

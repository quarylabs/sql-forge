use std::cell::RefCell;
use std::fmt::{self, Debug};
use std::ops::{Deref, Range};
use std::rc::Rc;
use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use itertools::chain;
use strum_macros::AsRefStr;

use super::context::RuleContext;
use super::crawlers::{BaseCrawler, Crawler};
use crate::core::config::{FluffConfig, Value};
use crate::core::dialects::base::Dialect;
use crate::core::dialects::init::DialectKind;
use crate::core::errors::SQLLintError;
use crate::core::parser::segments::base::{ErasedSegment, Tables};
use crate::core::templaters::base::{RawFileSlice, TemplatedFile};
use crate::helpers::{Config, IndexMap};

#[derive(Clone)]
pub struct LintResult {
    pub anchor: Option<ErasedSegment>,
    pub fixes: Vec<LintFix>,

    #[allow(dead_code)]
    memory: Option<AHashMap<String, String>>, // Adjust type as needed
    description: Option<String>,
    source: String,
}

#[derive(Debug, Clone, PartialEq, Copy, Hash, Eq, AsRefStr)]
#[strum(serialize_all = "lowercase")]
pub enum RuleGroups {
    All,
    Core,
    Aliasing,
    Ambiguous,
    Capitalisation,
    Convention,
    Layout,
    References,
    Structure,
}

impl LintResult {
    pub fn new(
        anchor: Option<ErasedSegment>,
        fixes: Vec<LintFix>,
        memory: Option<AHashMap<String, String>>,
        description: Option<String>,
        source: Option<String>,
    ) -> Self {
        // let fixes = fixes.into_iter().filter(|f| !f.is_trivial()).collect();

        LintResult { anchor, fixes, memory, description, source: source.unwrap_or_default() }
    }

    pub fn to_linting_error(&self, rule: ErasedRule) -> Option<SQLLintError> {
        let anchor = self.anchor.clone()?;

        let description =
            self.description.clone().unwrap_or_else(|| rule.description().to_string());

        SQLLintError::new(description.as_str(), anchor)
            .config(|this| this.rule = rule.into())
            .into()
    }
}

impl Debug for LintResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.anchor {
            None => write!(f, "LintResult(<empty>)"),
            Some(anchor) => {
                let fix_coda = if !self.fixes.is_empty() {
                    format!("+{}F", self.fixes.len())
                } else {
                    "".to_string()
                };

                match &self.description {
                    Some(desc) => {
                        if !self.source.is_empty() {
                            write!(
                                f,
                                "LintResult({} [{}]: {:?}{})",
                                desc, self.source, anchor, fix_coda
                            )
                        } else {
                            write!(f, "LintResult({}: {:?}{})", desc, anchor, fix_coda)
                        }
                    }
                    None => write!(f, "LintResult({:?}{})", anchor, fix_coda),
                }
            }
        }
    }
}

/// One of `create_before`, `create_after`, `replace`, `delete` to indicate the
/// kind of fix required.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditType {
    CreateBefore,
    CreateAfter,
    Replace,
    Delete,
}

/// A class to hold a potential fix to a linting violation.
///
///     Args:
///         edit_type (:obj:`str`): One of `create_before`, `create_after`,
///             `replace`, `delete` to indicate the kind of fix this represents.
///         anchor (:obj:`BaseSegment`): A segment which represents
///             the *position* that this fix should be applied at. For deletions
///             it represents the segment to delete, for creations it implies
/// the             position to create at (with the existing element at this
/// position             to be moved *after* the edit), for a `replace` it
/// implies the             segment to be replaced.
///         edit (iterable of :obj:`BaseSegment`, optional): For `replace` and
///             `create` fixes, this holds the iterable of segments to create
///             or replace at the given `anchor` point.
///         source (iterable of :obj:`BaseSegment`, optional): For `replace` and
///             `create` fixes, this holds iterable of segments that provided
///             code. IMPORTANT: The linter uses this to prevent copying
/// material             from templated areas.
#[derive(Debug, Clone)]
pub struct LintFix {
    pub edit_type: EditType,
    pub anchor: ErasedSegment,
    pub edit: Option<Vec<ErasedSegment>>,
    pub source: Vec<ErasedSegment>,
}

impl LintFix {
    fn new(
        edit_type: EditType,
        anchor: ErasedSegment,
        edit: Option<Vec<ErasedSegment>>,
        source: Option<Vec<ErasedSegment>>,
    ) -> Self {
        // If `edit` is provided, copy all elements and strip position markers.
        let mut clean_edit = None;
        if let Some(mut edit) = edit {
            // Developer Note: Ensure position markers are unset for all edit segments.
            // We rely on realignment to make position markers later in the process.
            for seg in &mut edit {
                if seg.get_position_marker().is_some() {
                    seg.make_mut().set_position_marker(None);
                };
            }
            clean_edit = Some(edit);
        }

        // If `source` is provided, filter segments with position markers.
        let clean_source = source.map_or(Vec::new(), |source| {
            source.into_iter().filter(|seg| seg.get_position_marker().is_some()).collect()
        });

        LintFix { edit_type, anchor, edit: clean_edit, source: clean_source }
    }

    pub fn create_before(anchor: ErasedSegment, edit_segments: Vec<ErasedSegment>) -> Self {
        Self::new(EditType::CreateBefore, anchor, edit_segments.into(), None)
    }

    pub fn create_after(
        anchor: ErasedSegment,
        edit_segments: Vec<ErasedSegment>,
        source: Option<Vec<ErasedSegment>>,
    ) -> Self {
        Self::new(EditType::CreateAfter, anchor, edit_segments.into(), source)
    }

    pub fn replace(
        anchor_segment: ErasedSegment,
        edit_segments: Vec<ErasedSegment>,
        source: Option<Vec<ErasedSegment>>,
    ) -> Self {
        Self::new(EditType::Replace, anchor_segment, Some(edit_segments), source)
    }

    pub fn delete(anchor_segment: ErasedSegment) -> Self {
        Self::new(EditType::Delete, anchor_segment, None, None)
    }

    /// Return whether this a valid source only edit.
    pub fn is_just_source_edit(&self) -> bool {
        if let Some(edit) = &self.edit {
            self.edit_type == EditType::Replace
                && edit.len() == 1
                && edit[0].raw() == self.anchor.raw()
        } else {
            false
        }
    }

    fn fix_slices(
        &self,
        templated_file: &TemplatedFile,
        within_only: bool,
    ) -> AHashSet<RawFileSlice> {
        let anchor_slice = self.anchor.get_position_marker().unwrap().templated_slice.clone();

        let adjust_boundary = if !within_only { 1 } else { 0 };

        let templated_slice = match self.edit_type {
            EditType::CreateBefore => anchor_slice.start - 1..anchor_slice.start + adjust_boundary,
            EditType::CreateAfter => anchor_slice.end - adjust_boundary..anchor_slice.end + 1,
            EditType::Replace => {
                let pos = self.anchor.get_position_marker().unwrap();
                if pos.source_slice.start == pos.source_slice.end {
                    return AHashSet::new();
                } else if self
                    .edit
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .all(|it| it.segments().is_empty() && !it.get_source_fixes().is_empty())
                {
                    let source_edit_slices: Vec<_> = self
                        .edit
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .flat_map(|edit| edit.get_source_fixes())
                        .map(|source_fixe| source_fixe.source_slice.clone())
                        .collect();

                    let slice =
                        templated_file.raw_slices_spanning_source_slice(&source_edit_slices[0]);
                    return AHashSet::from_iter(slice);
                }

                anchor_slice
            }
            _ => anchor_slice,
        };

        self.raw_slices_from_templated_slices(
            templated_file,
            std::iter::once(templated_slice),
            RawFileSlice::new(String::new(), "literal".to_string(), usize::MAX, None, None).into(),
        )
    }

    fn raw_slices_from_templated_slices(
        &self,
        templated_file: &TemplatedFile,
        templated_slices: impl Iterator<Item = Range<usize>>,
        file_end_slice: Option<RawFileSlice>,
    ) -> AHashSet<RawFileSlice> {
        let mut raw_slices = AHashSet::new();

        for templated_slice in templated_slices {
            let templated_slice =
                templated_file.templated_slice_to_source_slice(templated_slice.clone());

            match templated_slice {
                Ok(templated_slice) => raw_slices
                    .extend(templated_file.raw_slices_spanning_source_slice(&templated_slice)),
                Err(_) => {
                    if let Some(file_end_slice) = file_end_slice.clone() {
                        raw_slices.insert(file_end_slice);
                    }
                }
            }
        }

        raw_slices
    }

    pub fn has_template_conflicts(&self, templated_file: &TemplatedFile) -> bool {
        if self.edit_type == EditType::Replace
            && self.edit.is_none()
            && self.edit.as_ref().unwrap().len() == 1
        {
            let edit = &self.edit.as_ref().unwrap()[0];
            if edit.raw() == self.anchor.raw() && !edit.get_source_fixes().is_empty() {
                return false;
            }
        }

        let check_fn = if let EditType::CreateAfter | EditType::CreateBefore = self.edit_type {
            itertools::all
        } else {
            itertools::any
        };

        let fix_slices = self.fix_slices(templated_file, false);
        let result = check_fn(fix_slices, |fs: RawFileSlice| fs.slice_type == "templated");

        if result || self.source.is_empty() {
            return result;
        }

        let templated_slices = None;
        let raw_slices = self.raw_slices_from_templated_slices(
            templated_file,
            templated_slices.into_iter(),
            None,
        );
        raw_slices.iter().any(|fs| fs.slice_type == "templated")
    }
}

impl PartialEq for LintFix {
    fn eq(&self, other: &Self) -> bool {
        // Check if edit_types are equal
        if self.edit_type != other.edit_type {
            return false;
        }
        // Check if anchor.class_types are equal
        if self.anchor.get_type() != other.anchor.get_type() {
            return false;
        }
        // Check if anchor.uuids are equal
        if self.anchor.id() != other.anchor.id() {
            return false;
        }
        // Compare edits if they exist
        if let Some(self_edit) = &self.edit {
            if let Some(other_edit) = &other.edit {
                // Check lengths
                if self_edit.len() != other_edit.len() {
                    return false;
                }
                // Compare raw and source_fixes for each corresponding BaseSegment
                for (self_base_segment, other_base_segment) in self_edit.iter().zip(other_edit) {
                    if self_base_segment.raw() != other_base_segment.raw()
                        || self_base_segment.get_source_fixes()
                            != other_base_segment.get_source_fixes()
                    {
                        return false;
                    }
                }
            } else {
                // self has edit, other doesn't
                return false;
            }
        } else if other.edit.is_some() {
            // other has edit, self doesn't
            return false;
        }
        // If none of the above conditions were met, objects are equal
        true
    }
}

pub trait CloneRule {
    fn erased(&self) -> ErasedRule;
}

impl<T: Rule> CloneRule for T {
    fn erased(&self) -> ErasedRule {
        dyn_clone::clone(self).erased()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LintPhase {
    Main,
    Post,
}

pub trait Rule: CloneRule + dyn_clone::DynClone + Debug + 'static + Send + Sync {
    fn load_from_config(&self, _config: &AHashMap<String, Value>) -> Result<ErasedRule, String>;

    fn lint_phase(&self) -> LintPhase {
        LintPhase::Main
    }

    fn name(&self) -> &'static str;

    fn config_ref(&self) -> &'static str {
        self.name()
    }

    fn description(&self) -> &'static str;

    fn long_description(&self) -> &'static str;

    /// All the groups this rule belongs to, including 'all' because that is a
    /// given. There should be no duplicates and 'all' should be the first
    /// element.
    fn groups(&self) -> &'static [RuleGroups];

    fn force_enable(&self) -> bool {
        false
    }

    /// Returns the set of dialects for which a particular rule should be
    /// skipped.
    fn dialect_skip(&self) -> &'static [DialectKind] {
        &[]
    }

    fn code(&self) -> &'static str {
        let name = std::any::type_name::<Self>();
        name.split("::").last().unwrap().strip_prefix("Rule").unwrap_or(name)
    }

    fn eval(&self, rule_cx: RuleContext) -> Vec<LintResult>;

    fn is_fix_compatible(&self) -> bool {
        false
    }

    fn crawl_behaviour(&self) -> Crawler;

    fn crawl(
        &self,
        tables: &Tables,
        dialect: &Dialect,
        fix: bool,
        templated_file: &TemplatedFile,
        tree: ErasedSegment,
        config: &FluffConfig,
    ) -> (Vec<SQLLintError>, Vec<LintFix>) {
        let root_context = RuleContext {
            tables,
            dialect,
            fix,
            config: Some(config),
            segment: tree.clone(),
            templated_file: <_>::default(),
            path: <_>::default(),
            parent_stack: <_>::default(),
            raw_stack: <_>::default(),
            memory: Rc::new(RefCell::new(AHashMap::new())),
            segment_idx: 0,
        };
        let mut vs = Vec::new();
        let mut fixes = Vec::new();

        // TODO Will to return a note that rules were skipped
        if self.dialect_skip().contains(&dialect.name) && !self.force_enable() {
            return (Vec::new(), Vec::new());
        }

        for context in self.crawl_behaviour().crawl(root_context) {
            let resp =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.eval(context)));

            let resp = match resp {
                Ok(t) => t,
                Err(_) => {
                    vs.push(SQLLintError::new("Unexpected exception. Could you open an issue at https://github.com/quarylabs/sqruff", tree.clone()));
                    return (vs, fixes);
                }
            };

            let mut new_lerrs = Vec::new();
            let mut new_fixes = Vec::new();

            if resp.is_empty() {
                // Assume this means no problems (also means no memory)
            } else {
                for elem in resp {
                    self.process_lint_result(elem, templated_file, &mut new_lerrs, &mut new_fixes);
                }
            }

            // Consume the new results
            vs.extend(new_lerrs);
            fixes.extend(new_fixes);
        }

        (vs, fixes)
    }

    fn process_lint_result(
        &self,
        res: LintResult,
        templated_file: &TemplatedFile,
        new_lerrs: &mut Vec<SQLLintError>,
        new_fixes: &mut Vec<LintFix>,
    ) {
        if res.fixes.iter().any(|it| it.has_template_conflicts(templated_file)) {
            return;
        }

        let ignored = false;

        if let Some(lerr) = res.to_linting_error(self.erased()) {
            new_lerrs.push(lerr);
        }

        if !ignored {
            new_fixes.extend(res.fixes);
        }
    }
}

dyn_clone::clone_trait_object!(Rule);

#[derive(Debug, Clone)]
pub struct ErasedRule {
    erased: Arc<dyn Rule>,
}

impl PartialEq for ErasedRule {
    fn eq(&self, _other: &Self) -> bool {
        unimplemented!()
    }
}

impl Deref for ErasedRule {
    type Target = dyn Rule;

    fn deref(&self) -> &Self::Target {
        self.erased.as_ref()
    }
}

pub trait Erased {
    type Erased;

    fn erased(self) -> Self::Erased;
}

impl<T: Rule> Erased for T {
    type Erased = ErasedRule;

    fn erased(self) -> Self::Erased {
        ErasedRule { erased: Arc::new(self) }
    }
}

pub struct RuleManifest {
    pub code: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub groups: &'static [RuleGroups],
    pub aliases: Vec<&'static str>,
    pub rule_class: ErasedRule,
}

#[derive(Clone)]
pub struct RulePack {
    pub(crate) rules: Vec<ErasedRule>,
    _reference_map: AHashMap<&'static str, AHashSet<&'static str>>,
}

impl RulePack {
    pub fn rules(&self) -> Vec<ErasedRule> {
        self.rules.clone()
    }
}

pub struct RuleSet {
    pub(crate) register: IndexMap<&'static str, RuleManifest>,
}

impl RuleSet {
    fn rule_reference_map(&self) -> AHashMap<&'static str, AHashSet<&'static str>> {
        let valid_codes: AHashSet<_> = self.register.keys().cloned().collect();

        let reference_map: AHashMap<_, AHashSet<_>> =
            valid_codes.iter().map(|&code| (code, AHashSet::from([code]))).collect();

        let name_map = {
            let mut name_map = AHashMap::new();
            for manifest in self.register.values() {
                name_map.entry(manifest.name).or_insert_with(AHashSet::new).insert(manifest.code);
            }
            name_map
        };

        let name_collisions: AHashSet<_> = {
            let name_keys: AHashSet<_> = name_map.keys().cloned().collect();
            name_keys.intersection(&valid_codes).copied().collect()
        };

        if !name_collisions.is_empty() {
            tracing::warn!(
                "The following defined rule names were found which collide with codes. Those \
                 names will not be available for selection: {name_collisions:?}",
            );
        }

        let reference_map: AHashMap<_, _> = chain(name_map, reference_map).collect();

        let mut group_map: AHashMap<_, AHashSet<&'static str>> = AHashMap::new();
        for manifest in self.register.values() {
            for group in manifest.groups {
                let group = group.as_ref();
                if let Some(codes) = reference_map.get(group) {
                    tracing::warn!(
                        "Rule {} defines group '{}' which is already defined as a name or code of \
                         {:?}. This group will not be available for use as a result of this \
                         collision.",
                        manifest.code,
                        group,
                        codes
                    );
                } else {
                    group_map.entry(group).or_insert_with(AHashSet::new).insert(manifest.code);
                }
            }
        }

        let reference_map: AHashMap<_, _> = chain(group_map, reference_map).collect();

        let mut alias_map: AHashMap<_, AHashSet<&'static str>> = AHashMap::new();
        for manifest in self.register.values() {
            for alias in &manifest.aliases {
                if let Some(codes) = reference_map.get(alias) {
                    tracing::warn!(
                        "Rule {} defines alias '{}' which is already defined as a name, code or \
                         group of {:?}. This alias will not be available for use as a result of \
                         this collision.",
                        manifest.code,
                        alias,
                        codes
                    );
                } else {
                    alias_map.entry(*alias).or_insert_with(AHashSet::new).insert(manifest.code);
                }
            }
        }

        chain(alias_map, reference_map).collect()
    }

    fn expand_rule_refs(
        &self,
        glob_list: Vec<String>,
        reference_map: &AHashMap<&'static str, AHashSet<&'static str>>,
    ) -> AHashSet<&'static str> {
        let mut expanded_rule_set = AHashSet::new();

        for r in glob_list {
            if reference_map.contains_key(r.as_str()) {
                expanded_rule_set.extend(reference_map[r.as_str()].clone());
            } else {
                panic!("Rule {r} not found in rule reference map");
            }
        }

        expanded_rule_set
    }

    pub(crate) fn get_rulepack(&self, config: &FluffConfig) -> RulePack {
        let reference_map = self.rule_reference_map();
        let rules = config.get_section("rules");
        let keylist = self.register.keys();
        let mut instantiated_rules = Vec::with_capacity(keylist.len());

        let allowlist: Vec<String> = match config.get("rule_allowlist", "core").as_array() {
            Some(array) => array.iter().map(|it| it.as_string().unwrap().to_owned()).collect(),
            None => self.register.keys().map(|it| it.to_string()).collect(),
        };

        let denylist: Vec<String> = match config.get("rule_denylist", "core").as_array() {
            Some(array) => array.iter().map(|it| it.as_string().unwrap().to_owned()).collect(),
            None => Vec::new(),
        };

        let expanded_allowlist = self.expand_rule_refs(allowlist, &reference_map);
        let expanded_denylist = self.expand_rule_refs(denylist, &reference_map);

        let keylist: Vec<_> = keylist
            .into_iter()
            .filter(|&&r| expanded_allowlist.contains(r) && !expanded_denylist.contains(r))
            .collect();

        for code in keylist {
            let rule = self.register[code].rule_class.clone();
            let rule_config_ref = rule.config_ref();

            let tmp = AHashMap::new();

            let specific_rule_config =
                rules.get(rule_config_ref).and_then(|section| section.as_map()).unwrap_or(&tmp);

            // TODO fail the rulepack if any need unwrapping
            instantiated_rules.push(rule.load_from_config(specific_rule_config).unwrap());
        }

        RulePack { rules: instantiated_rules, _reference_map: reference_map }
    }
}

use std::fmt::{self, Debug};
use std::ops::Deref;

use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use itertools::chain;
use sqruff_lib_core::dialects::base::Dialect;
use sqruff_lib_core::dialects::init::DialectKind;
use sqruff_lib_core::errors::{ErrorStructRule, SQLLintError};
use sqruff_lib_core::helpers::{Config, IndexMap};
use sqruff_lib_core::lint_fix::LintFix;
use sqruff_lib_core::parser::segments::base::{ErasedSegment, Tables};
use sqruff_lib_core::templaters::base::TemplatedFile;
use strum_macros::AsRefStr;

use super::context::RuleContext;
use super::crawlers::{BaseCrawler, Crawler};
use crate::core::config::{FluffConfig, Value};

pub struct LintResult {
    pub anchor: Option<ErasedSegment>,
    pub fixes: Vec<LintFix>,
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
    Jinja,
    Layout,
    References,
    Structure,
}

impl LintResult {
    pub fn new(
        anchor: Option<ErasedSegment>,
        fixes: Vec<LintFix>,
        description: Option<String>,
        source: Option<String>,
    ) -> Self {
        // let fixes = fixes.into_iter().filter(|f| !f.is_trivial()).collect();

        LintResult {
            anchor,
            fixes,
            description,
            source: source.unwrap_or_default(),
        }
    }

    pub fn to_linting_error(&self, rule: ErasedRule, fixes: Vec<LintFix>) -> Option<SQLLintError> {
        let anchor = self.anchor.clone()?;

        let description = self
            .description
            .clone()
            .unwrap_or_else(|| rule.description().to_string());

        let is_fixable = rule.is_fix_compatible();

        SQLLintError::new(description.as_str(), anchor, is_fixable, fixes)
            .config(|this| {
                this.rule = Some(ErrorStructRule {
                    name: rule.name(),
                    code: rule.code(),
                })
            })
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
        name.split("::")
            .last()
            .unwrap()
            .strip_prefix("Rule")
            .unwrap_or(name)
    }

    fn eval(&self, rule_cx: &RuleContext) -> Vec<LintResult>;

    fn is_fix_compatible(&self) -> bool {
        false
    }

    fn crawl_behaviour(&self) -> Crawler;

    fn crawl(
        &self,
        tables: &Tables,
        dialect: &Dialect,
        templated_file: &TemplatedFile,
        tree: ErasedSegment,
        config: &FluffConfig,
    ) -> Vec<SQLLintError> {
        let mut root_context =
            RuleContext::new(tables, dialect, config, tree.clone(), &templated_file);
        let mut vs = Vec::new();

        // TODO Will to return a note that rules were skipped
        if self.dialect_skip().contains(&dialect.name) && !self.force_enable() {
            return Vec::new();
        }

        self.crawl_behaviour().crawl(&mut root_context, &mut |context| {
            let resp =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.eval(context)));

            let resp = match resp {
                Ok(t) => t,
                Err(_) => {
                    vs.push(SQLLintError::new("Unexpected exception. Could you open an issue at https://github.com/quarylabs/sqruff", tree.clone(), false, vec![]));
                    Vec::new()
                }
            };

            let mut new_lerrs = Vec::new();

            if resp.is_empty() {
                // Assume this means no problems (also means no memory)
            } else {
                for elem in resp {
                    self.process_lint_result(elem, templated_file, &mut new_lerrs);
                }
            }

            // Consume the new results
            vs.extend(new_lerrs);
        });

        vs
    }

    fn process_lint_result(
        &self,
        res: LintResult,
        templated_file: &TemplatedFile,
        new_lerrs: &mut Vec<SQLLintError>,
    ) {
        if res
            .fixes
            .iter()
            .any(|it| it.has_template_conflicts(templated_file))
        {
            return;
        }

        if let Some(lerr) = res.to_linting_error(self.erased(), res.fixes.clone()) {
            new_lerrs.push(lerr);
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
        ErasedRule {
            erased: Arc::new(self),
        }
    }
}

pub struct RuleManifest {
    pub code: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub groups: &'static [RuleGroups],
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
        let valid_codes: AHashSet<_> = self.register.keys().copied().collect();

        let reference_map: AHashMap<_, AHashSet<_>> = valid_codes
            .iter()
            .map(|&code| (code, AHashSet::from([code])))
            .collect();

        let name_map = {
            let mut name_map = AHashMap::new();
            for manifest in self.register.values() {
                name_map
                    .entry(manifest.name)
                    .or_insert_with(AHashSet::new)
                    .insert(manifest.code);
            }
            name_map
        };

        let name_collisions: AHashSet<_> = {
            let name_keys: AHashSet<_> = name_map.keys().copied().collect();
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
                    group_map
                        .entry(group)
                        .or_insert_with(AHashSet::new)
                        .insert(manifest.code);
                }
            }
        }

        chain(group_map, reference_map).collect()
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
            Some(array) => array
                .iter()
                .map(|it| it.as_string().unwrap().to_owned())
                .collect(),
            None => self.register.keys().map(|it| it.to_string()).collect(),
        };

        let denylist: Vec<String> = match config.get("rule_denylist", "core").as_array() {
            Some(array) => array
                .iter()
                .map(|it| it.as_string().unwrap().to_owned())
                .collect(),
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

            let specific_rule_config = rules
                .get(rule_config_ref)
                .and_then(|section| section.as_map())
                .unwrap_or(&tmp);

            // TODO fail the rulepack if any need unwrapping
            instantiated_rules.push(rule.load_from_config(specific_rule_config).unwrap());
        }

        RulePack {
            rules: instantiated_rules,
            _reference_map: reference_map,
        }
    }
}

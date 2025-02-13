use itertools::{chain, Itertools};
use sqruff_lib_core::helpers::IndexMap;

use crate::core::rules::base::{ErasedRule, RuleManifest, RuleSet};

pub mod aliasing;
pub mod ambiguous;
pub mod capitalisation;
pub mod convention;
pub mod jinja;
pub mod layout;
pub mod references;
pub mod structure;

pub fn rules() -> Vec<ErasedRule> {
    chain!(
        aliasing::rules(),
        ambiguous::rules(),
        capitalisation::rules(),
        convention::rules(),
        layout::rules(),
        references::rules(),
        structure::rules(),
        jinja::rules(),
    )
    .collect_vec()
}

pub fn get_ruleset() -> RuleSet {
    let mut register = IndexMap::default();

    let rules = rules();
    register.reserve(rules.len());

    for rule in rules {
        register.insert(
            rule.code(),
            RuleManifest {
                code: rule.code(),
                name: rule.name(),
                description: rule.description(),
                groups: rule.groups(),
                rule_class: rule,
            },
        );
    }

    RuleSet { register }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rules::base::RuleGroups;

    #[test]
    fn no_rule_should_not_include_all_as_that_is_default() {
        rules().iter().for_each(|rule| {
            assert_eq!(*rule.groups().first().unwrap(), RuleGroups::All);
        });
    }

    #[test]
    fn no_should_contain_duplicate_groups() {
        rules().iter().for_each(|rule| {
            let groups = rule.groups();
            assert_eq!(groups.len(), groups.iter().unique().count());
        });
    }

    #[test]
    fn if_rule_contains_core_is_second_in_list() {
        rules().iter().for_each(|rule| {
            let groups = rule.groups();
            if groups.iter().any(|&rule| rule == RuleGroups::Core) {
                assert_eq!(groups.get(1).unwrap(), &RuleGroups::Core);
            }
        })
    }

    #[test]
    fn rule_skip_dialect_should_have_no_duplicates() {
        rules().iter().for_each(|rule| {
            let skips = rule.dialect_skip();
            assert_eq!(skips.len(), skips.iter().unique().count());
        })
    }

    #[test]
    fn rule_skip_dialect_should_be_alphabetical() {
        rules().iter().for_each(|rule| {
            let skips = rule.dialect_skip();
            for i in 1..skips.len() {
                if skips[i].as_ref() < skips[i].as_ref() {
                    panic!("not in alphabetical order in rule {}", rule.code())
                }
            }
        })
    }
}

use crate::core::rules::base::ErasedRule;

pub mod jj01;

pub fn rules() -> Vec<ErasedRule> {
    use crate::core::rules::base::Erased as _;

    vec![jj01::RuleJJ01.erased()]
}

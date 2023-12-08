use std::collections::HashSet;

use crate::core::parser::{
    context::ParseContext, match_result::MatchResult, matchable::Matchable, segments::base::Segment,
};

#[derive(Debug, Clone, PartialEq)]
pub struct NonCodeMatcher;

impl Matchable for NonCodeMatcher {
    // Implement the simple method
    fn simple(
        &self,
        parse_context: &ParseContext,
        crumbs: Option<Vec<&str>>,
    ) -> Option<(HashSet<String>, HashSet<String>)> {
        None
    }

    fn is_optional(&self) -> bool {
        // Not optional
        false
    }

    fn cache_key(&self) -> String {
        "non-code-matcher".to_string()
    }

    fn match_segments(
        &self,
        segments: Vec<Box<dyn Segment>>,
        _parse_context: &mut ParseContext,
    ) -> MatchResult {
        // Match any starting non-code segments
        let mut idx = 0;

        while idx < segments.len() && !segments[idx].is_code() {
            idx += 1;
        }

        MatchResult::new(segments[0..idx].to_vec(), segments[idx..].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use crate::core::{
        dialects::init::{dialect_selector, get_default_dialect},
        parser::{
            context::ParseContext, grammar::noncode::NonCodeMatcher, matchable::Matchable,
            segments::test_functions::test_segments,
        },
    };

    #[test]
    fn test_non_code_matcher() {
        let dialect = dialect_selector(get_default_dialect()).unwrap(); // Assuming this function exists and returns a Dialect
        let mut ctx = ParseContext::new(dialect);

        let matcher = NonCodeMatcher;
        let test_segments = test_segments(); // Assuming this function exists and generates test segments
        let m = matcher.match_segments(test_segments[1..].to_vec(), &mut ctx);

        // NonCode Matcher doesn't work with simple
        assert!(matcher.simple(&ctx, None).is_none());

        // We should match one and only one segment
        assert_eq!(m.len(), 1);
    }
}

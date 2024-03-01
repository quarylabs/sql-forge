use std::collections::HashSet;

use crate::core::parser::segments::base::Segment;
use crate::core::rules::base::{LintResult, Rule};
use crate::core::rules::context::RuleContext;
use crate::core::rules::crawlers::{Crawler, SegmentSeekerCrawler};
use crate::utils::reflow::sequence::ReflowSequence;

#[derive(Debug, Default, Clone)]
pub struct RuleLT03 {}

impl Rule for RuleLT03 {
    fn name(&self) -> &'static str {
        "layout.operators"
    }

    fn crawl_behaviour(&self) -> Crawler {
        SegmentSeekerCrawler::new(HashSet::from(["binary_operator", "comparison_operator"])).into()
    }

    fn eval(&self, context: RuleContext) -> Vec<LintResult> {
        if context.segment.is_type("") {
            unimplemented!()
        } else if context.segment.is_type("binary_operator") {
            let binary_positioning = "leading";
            if self.check_trail_lead_shortcut(
                context.segment.as_ref(),
                context.parent_stack.last().unwrap().as_ref(),
                binary_positioning,
            ) {
                return Vec::new();
            }
        } else {
            unimplemented!()
        }

        ReflowSequence::from_around_target(
            &context.segment,
            context.parent_stack.first().unwrap().clone_box(),
            "both",
        )
        .rebreak()
        .results()
    }
}

impl RuleLT03 {
    pub(crate) fn check_trail_lead_shortcut(
        &self,
        segment: &dyn Segment,
        parent: &dyn Segment,
        line_position: &str,
    ) -> bool {
        let idx = parent.get_segments().iter().position(|it| it.dyn_eq(segment)).unwrap();

        // Shortcut #1: Leading.
        if line_position == "leading" {
            if self.seek_newline(&parent.get_segments(), idx, -1) {
                return true;
            }
            // If we didn't find a newline before, if there's _also_ not a newline
            // after, then we can also shortcut. i.e., it's a comma "mid line".
            if !self.seek_newline(&parent.get_segments(), idx, 1) {
                return true;
            }
        }
        // Shortcut #2: Trailing.
        else if line_position == "trailing" {
            if self.seek_newline(&parent.get_segments(), idx, 1) {
                return true;
            }
            // If we didn't find a newline after, if there's _also_ not a newline
            // before, then we can also shortcut. i.e., it's a comma "mid line".
            if !self.seek_newline(&parent.get_segments(), idx, -1) {
                return true;
            }
        }

        false
    }

    fn seek_newline(&self, segments: &[Box<dyn Segment>], idx: usize, dir: i32) -> bool {
        assert!(dir == 1 || dir == -1, "Direction must be 1 or -1");

        let range = if dir == 1 { idx + 1..segments.len() } else { 0..idx };

        for segment in segments[range].iter().step_by(dir.abs() as usize) {
            if segment.is_type("newline") {
                return true;
            } else if !segment.is_type("whitespace")
                && !segment.is_type("indent")
                && !segment.is_type("comment")
            {
                break;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::RuleLT03;
    use crate::api::simple::{fix, lint};
    use crate::core::rules::base::Erased;

    #[test]
    fn passes_on_before_default() {
        let sql = r#"
select
    a
    + b
from foo
"#;

        let result =
            lint(sql.into(), "ansi".into(), vec![RuleLT03::default().erased()], None, None)
                .unwrap();

        assert_eq!(result, &[]);
    }

    #[test]
    fn fails_on_after_default() {
        let sql = r#"
select
    a +
    b
from foo
"#;

        let result = fix(sql.into(), vec![RuleLT03::default().erased()]);
        println!("{}", result);
    }
}

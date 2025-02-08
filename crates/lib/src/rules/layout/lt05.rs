use ahash::{AHashMap, AHashSet};
use itertools::enumerate;
use sqruff_lib_core::dialects::syntax::SyntaxKind;

use crate::core::config::Value;
use crate::core::rules::base::{Erased, ErasedRule, LintResult, Rule, RuleGroups};
use crate::core::rules::context::RuleContext;
use crate::core::rules::crawlers::{Crawler, RootOnlyCrawler};
use crate::utils::reflow::sequence::ReflowSequence;

#[derive(Debug, Default, Clone)]
pub struct RuleLT05 {
    ignore_comment_lines: bool,
    ignore_comment_clauses: bool,
}

impl Rule for RuleLT05 {
    fn load_from_config(&self, config: &AHashMap<String, Value>) -> Result<ErasedRule, String> {
        Ok(RuleLT05 {
            ignore_comment_lines: config["ignore_comment_lines"].as_bool().unwrap(),
            ignore_comment_clauses: config["ignore_comment_clauses"].as_bool().unwrap(),
        }
        .erased())
    }
    fn name(&self) -> &'static str {
        "layout.long_lines"
    }

    fn description(&self) -> &'static str {
        "Line is too long."
    }

    fn long_description(&self) -> &'static str {
        r#"
**Anti-pattern**

In this example, the line is too long.

```sql
SELECT
    my_function(col1 + col2, arg2, arg3) over (partition by col3, col4 order by col5 rows between unbounded preceding and current row) as my_relatively_long_alias,
    my_other_function(col6, col7 + col8, arg4) as my_other_relatively_long_alias,
    my_expression_function(col6, col7 + col8, arg4) = col9 + col10 as another_relatively_long_alias
FROM my_table
```

**Best practice**

Wraps the line to be within the maximum line length.

```sql
SELECT
    my_function(col1 + col2, arg2, arg3)
        over (
            partition by col3, col4
            order by col5 rows between unbounded preceding and current row
        )
        as my_relatively_long_alias,
    my_other_function(col6, col7 + col8, arg4)
        as my_other_relatively_long_alias,
    my_expression_function(col6, col7 + col8, arg4)
    = col9 + col10 as another_relatively_long_alias
FROM my_table"#
    }

    fn groups(&self) -> &'static [RuleGroups] {
        &[RuleGroups::All, RuleGroups::Core, RuleGroups::Layout]
    }
    fn eval(&self, context: &RuleContext) -> Vec<LintResult> {
        let mut results = ReflowSequence::from_root(context.segment.clone(), context.config)
            .break_long_lines(context.tables)
            .results();

        let mut to_remove = AHashSet::new();

        if self.ignore_comment_lines {
            let raw_segments = context.segment.get_raw_segments();
            for (res_idx, res) in enumerate(&results) {
                if res.anchor.as_ref().unwrap().is_type(SyntaxKind::Comment)
                    || res
                        .anchor
                        .as_ref()
                        .unwrap()
                        .is_type(SyntaxKind::InlineComment)
                {
                    to_remove.insert(res_idx);
                    continue;
                }

                let pos_marker = res.anchor.as_ref().unwrap().get_position_marker().unwrap();
                let raw_idx = raw_segments
                    .iter()
                    .position(|it| it == res.anchor.as_ref().unwrap())
                    .unwrap();

                for seg in &raw_segments[raw_idx..] {
                    if seg.get_position_marker().unwrap().working_line_no
                        != pos_marker.working_line_no
                    {
                        break;
                    }

                    if seg.is_type(SyntaxKind::Comment) || seg.is_type(SyntaxKind::InlineComment) {
                        to_remove.insert(res_idx);
                        break;
                    } else if seg.is_type(SyntaxKind::Placeholder) {
                        unimplemented!()
                    }
                }
            }
        }

        if self.ignore_comment_clauses {
            let raw_segments = context.segment.get_raw_segments();
            for (res_idx, res) in enumerate(&results) {
                let raw_idx = raw_segments
                    .iter()
                    .position(|it| it == res.anchor.as_ref().unwrap())
                    .unwrap();

                for seg in &raw_segments[raw_idx..] {
                    if seg.get_position_marker().unwrap().working_line_no
                        != res
                            .anchor
                            .as_ref()
                            .unwrap()
                            .get_position_marker()
                            .unwrap()
                            .working_line_no
                    {
                        break;
                    }

                    let mut is_break = false;

                    for ps in context.segment.path_to(seg) {
                        if ps.segment.is_type(SyntaxKind::CommentClause)
                            || ps.segment.is_type(SyntaxKind::CommentEqualsClause)
                        {
                            let line_pos =
                                ps.segment.get_position_marker().unwrap().working_line_pos;
                            if (line_pos as i32)
                                < context
                                    .config
                                    .get("max_line_length", "core")
                                    .as_int()
                                    .unwrap()
                            {
                                to_remove.insert(res_idx);
                                is_break = true;
                                break;
                            }
                        }
                    }

                    if is_break {
                        break;
                    } else {
                        continue;
                    }
                }
            }
        }

        for idx in to_remove {
            if idx < results.len() {
                results.remove(idx);
            } else {
                println!(
                    "Attempted to remove index {} from results with length {}",
                    idx,
                    results.len()
                );
            }
        }

        results
    }

    fn is_fix_compatible(&self) -> bool {
        true
    }

    fn crawl_behaviour(&self) -> Crawler {
        RootOnlyCrawler.into()
    }
}

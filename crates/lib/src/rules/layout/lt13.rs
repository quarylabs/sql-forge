use ahash::AHashMap;
use itertools::Itertools;

use crate::core::config::Value;
use crate::core::rules::base::{
    Erased, ErasedRule, LintFix, LintPhase, LintResult, Rule, RuleGroups,
};
use crate::core::rules::context::RuleContext;
use crate::core::rules::crawlers::{Crawler, RootOnlyCrawler};
use crate::dialects::SyntaxKind;
use crate::utils::functional::segments::Segments;

#[derive(Debug, Default, Clone)]
pub struct RuleLT13;

impl Rule for RuleLT13 {
    fn load_from_config(&self, _config: &AHashMap<String, Value>) -> Result<ErasedRule, String> {
        Ok(RuleLT13.erased())
    }

    fn lint_phase(&self) -> LintPhase {
        LintPhase::Post
    }

    fn name(&self) -> &'static str {
        "layout.start_of_file"
    }

    fn description(&self) -> &'static str {
        "Files must not begin with newlines or whitespace."
    }

    fn long_description(&self) -> &'static str {
        r#"
**Anti-pattern**

The file begins with newlines or whitespace. The ^ represents the beginning of the file.

```sql
 ^

 SELECT
     a
 FROM foo

 -- Beginning on an indented line is also forbidden,
 -- (the • represents space).

 ••••SELECT
 ••••a
 FROM
 ••••foo
```

**Best practice**

Start file on either code or comment. (The ^ represents the beginning of the file.)

```sql
 ^SELECT
     a
 FROM foo

 -- Including an initial block comment.

 ^/*
 This is a description of my SQL code.
 */
 SELECT
     a
 FROM
     foo

 -- Including an initial inline comment.

 ^--This is a description of my SQL code.
 SELECT
     a
 FROM
     foo
```
"#
    }

    fn groups(&self) -> &'static [RuleGroups] {
        &[RuleGroups::All, RuleGroups::Layout]
    }

    fn eval(&self, context: RuleContext) -> Vec<LintResult> {
        let mut raw_segments = Vec::new();

        for seg in context.segment.recursive_crawl_all(false) {
            if !seg.segments().is_empty() {
                continue;
            }

            if matches!(
                seg.get_type(),
                SyntaxKind::Newline
                    | SyntaxKind::Whitespace
                    | SyntaxKind::Indent
                    | SyntaxKind::Dedent
            ) {
                raw_segments.push(seg);
                continue;
            }

            let raw_stack =
                Segments::from_vec(raw_segments.clone(), context.templated_file.clone());
            // Non-whitespace segment.
            if !raw_stack.all(Some(|seg| seg.is_meta())) {
                return vec![LintResult::new(
                    context.segment.into(),
                    raw_stack.into_iter().map(LintFix::delete).collect_vec(),
                    None,
                    None,
                    None,
                )];
            } else {
                break;
            }
        }

        Vec::new()
    }

    fn is_fix_compatible(&self) -> bool {
        true
    }

    fn crawl_behaviour(&self) -> Crawler {
        RootOnlyCrawler.into()
    }
}

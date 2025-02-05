use ahash::AHashMap;
use sqruff_lib_core::dialects::syntax::{SyntaxKind, SyntaxSet};
use sqruff_lib_core::parser::segments::base::SegmentBuilder;

use crate::core::config::Value;
use crate::core::rules::base::{Erased, ErasedRule, LintResult, Rule, RuleGroups};
use crate::core::rules::context::RuleContext;
use crate::core::rules::crawlers::{Crawler, SegmentSeekerCrawler};
use crate::utils::reflow::sequence::{Filter, ReflowInsertPosition, ReflowSequence, TargetSide};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Aliasing {
    Explicit,
    Implicit,
}

#[derive(Debug, Clone)]
pub struct RuleJJ01 {
}

impl Rule for RuleJJ01 {
    fn load_from_config(&self, _config: &AHashMap<String, Value>) -> Result<ErasedRule, String> {
        Ok()
    }

    fn name(&self) -> &'static str {
        "jinja.padding"
    }

    fn description(&self) -> &'static str {
        "Jinja tags should have a single whitespace on either side."
    }

    fn long_description(&self) -> &'static str {
        r#"
**Anti-pattern**

Jinja tags with either no whitespace or very long whitespace are hard to read.

```sql
SELECT {{    a     }} from {{ref('foo')}}
```

**Best practice**

A single whitespace surrounding Jinja tags, alternatively longer gaps containing newlines are acceptable.

```sql
SELECT {{ a }} from {{ ref('foo') }};
SELECT {{ a }} from {{
     ref('foo')
}};
```
"#
    }

    fn groups(&self) -> &'static [RuleGroups] {
        &[RuleGroups::All, RuleGroups::Jinja]
    }

    fn eval(&self, rule_cx: &RuleContext) -> Vec<LintResult> {
        unimplemented!()
    }

    fn is_fix_compatible(&self) -> bool {
        true
    }

    fn crawl_behaviour(&self) -> Crawler {
        SegmentSeekerCrawler::new(const { SyntaxSet::new(&[SyntaxKind::AliasExpression]) }).into()
    }
}

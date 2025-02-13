use sqruff_lib_core::utils::functional::segments::Segments;

use crate::core::rules::context::RuleContext;

pub struct FunctionalContext<'a> {
    context: &'a RuleContext<'a>,
}

impl<'a> FunctionalContext<'a> {
    pub fn new(context: &'a RuleContext<'a>) -> Self {
        FunctionalContext { context }
    }

    pub fn segment(&self) -> Segments {
        Segments::new(
            self.context.segment.clone(),
            Some(self.context.templated_file.clone()),
        )
    }

    pub fn siblings_post(&self) -> Segments {
        Segments::from_vec(
            self.context.siblings_post(),
            Some(self.context.templated_file.clone()),
        )
    }

    pub fn parent_stack(&self) -> Segments {
        Segments::from_vec(
            self.context.parent_stack.clone(),
            Some(self.context.templated_file.clone()),
        )
    }
}

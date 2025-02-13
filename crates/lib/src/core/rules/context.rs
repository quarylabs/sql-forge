use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::rc::Rc;

use ahash::AHashMap;
use sqruff_lib_core::dialects::base::Dialect;
use sqruff_lib_core::parser::segments::base::{ErasedSegment, Tables};
use sqruff_lib_core::templaters::base::TemplatedFile;

use crate::core::config::FluffConfig;

#[derive(Debug)]
pub struct RuleContext<'a> {
    pub tables: &'a Tables,
    pub dialect: &'a Dialect,
    pub templated_file: &'a TemplatedFile,
    pub path: Option<String>,
    pub config: &'a FluffConfig,

    // These change within a file.
    /// segment: The segment in question
    pub segment: ErasedSegment,
    /// parent_stack: A tuple of the path from the root to this segment.
    pub parent_stack: Vec<ErasedSegment>,
    /// raw_stack: All the raw segments so far in the file
    pub raw_stack: Vec<ErasedSegment>,
    /// memory: Arbitrary storage for the rule
    pub memory: Rc<RefCell<AHashMap<TypeId, Box<dyn Any>>>>,
    /// segment_idx: The index of this segment in the parent
    pub segment_idx: usize,
}

pub struct Checkpoint {
    parent_stack: usize,
    raw_stack: usize,
}

impl<'a> RuleContext<'a> {
    pub fn new(
        tables: &'a Tables,
        dialect: &'a Dialect,
        config: &'a FluffConfig,
        segment: ErasedSegment,
        templated_file: &'a TemplatedFile,
    ) -> Self {
        Self {
            tables,
            dialect,
            config,
            segment,
            templated_file,
            path: <_>::default(),
            parent_stack: <_>::default(),
            raw_stack: <_>::default(),
            memory: Rc::new(RefCell::new(AHashMap::new())),
            segment_idx: 0,
        }
    }

    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            parent_stack: self.parent_stack.len(),
            raw_stack: self.raw_stack.len(),
        }
    }

    pub fn restore(&mut self, checkpoint: Checkpoint) {
        self.parent_stack.truncate(checkpoint.parent_stack);
        self.raw_stack.truncate(checkpoint.raw_stack);
    }

    pub fn try_get<T: Clone + 'static>(&self) -> Option<T> {
        let id = TypeId::of::<T>();

        let memory = self.memory.borrow();
        let value = memory.get(&id)?;
        let value = value.downcast_ref::<T>()?;

        Some(value.clone())
    }

    pub fn set<T: 'static>(&self, value: T) {
        let id = TypeId::of::<T>();
        self.memory.borrow_mut().insert(id, Box::new(value));
    }

    pub fn siblings_post(&self) -> Vec<ErasedSegment> {
        if !self.parent_stack.is_empty() {
            self.parent_stack.last().unwrap().segments()[self.segment_idx + 1..].to_vec()
        } else {
            Vec::new()
        }
    }
}

use std::iter::zip;

use ahash::{AHashMap, AHashSet};
use nohash_hasher::{IntMap, IntSet};
use uuid::Uuid;

use crate::core::parser::segments::base::{ErasedSegment, PathStep};

/// An element of the stack_positions property of DepthInfo.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StackPosition {
    pub idx: usize,
    pub len: usize,
    pub type_: &'static str,
}

impl StackPosition {
    /// Interpret a path step for stack_positions.
    fn stack_pos_interpreter(path_step: &PathStep) -> &'static str {
        if path_step.code_idxs.is_empty() {
            ""
        } else if path_step.code_idxs.len() == 1 {
            "solo"
        } else if path_step.idx == *path_step.code_idxs.first().unwrap() {
            "start"
        } else if path_step.idx == *path_step.code_idxs.last().unwrap() {
            "end"
        } else {
            ""
        }
    }

    /// Interpret a PathStep to construct a StackPosition
    fn from_path_step(path_step: &PathStep) -> StackPosition {
        StackPosition {
            idx: path_step.idx,
            len: path_step.len,
            type_: StackPosition::stack_pos_interpreter(path_step),
        }
    }
}

#[derive(Clone)]
pub struct DepthMap {
    depth_info: AHashMap<Uuid, DepthInfo>,
}

impl DepthMap {
    fn new(raws_with_stack: Vec<(ErasedSegment, Vec<PathStep>)>) -> Self {
        let mut depth_info = AHashMap::with_capacity(raws_with_stack.len());

        for (raw, stack) in raws_with_stack {
            depth_info.insert(raw.get_uuid(), DepthInfo::from_raw_and_stack(&raw, stack));
        }

        Self { depth_info }
    }

    pub fn get_depth_info(&self, seg: &ErasedSegment) -> DepthInfo {
        self.depth_info[&seg.get_uuid()].clone()
    }

    pub fn copy_depth_info(
        &mut self,
        anchor: &ErasedSegment,
        new_segment: &ErasedSegment,
        trim: u32,
    ) {
        self.depth_info.insert(
            new_segment.get_uuid(),
            self.get_depth_info(anchor).trim(trim.try_into().unwrap()),
        );
    }

    pub fn from_parent(parent: &ErasedSegment) -> Self {
        Self::new(parent.raw_segments_with_ancestors())
    }

    pub fn from_raws_and_root(
        raw_segments: Vec<ErasedSegment>,
        root_segment: &ErasedSegment,
    ) -> DepthMap {
        let mut buff = Vec::new();

        for raw in raw_segments {
            let stack = root_segment.path_to(&raw);
            buff.push((raw.clone(), stack));
        }

        DepthMap::new(buff)
    }
}

/// An object to hold the depth information for a specific raw segment.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct DepthInfo {
    pub stack_depth: usize,
    pub stack_hashes: Vec<u64>,
    /// This is a convenience cache to speed up operations.
    pub stack_hash_set: IntSet<u64>,
    pub stack_class_types: Vec<AHashSet<&'static str>>,
    pub stack_positions: IntMap<u64, StackPosition>,
}

impl DepthInfo {
    #[allow(unused_variables)]
    fn from_raw_and_stack(raw: &ErasedSegment, stack: Vec<PathStep>) -> DepthInfo {
        let stack_hashes: Vec<u64> = stack.iter().map(|ps| ps.segment.hash_value()).collect();
        let stack_hash_set: IntSet<u64> = IntSet::from_iter(stack_hashes.clone());

        let stack_class_types: Vec<AHashSet<&str>> =
            stack.iter().map(|ps| ps.segment.class_types()).collect();

        let stack_positions: IntMap<u64, StackPosition> = zip(stack_hashes.iter(), stack.iter())
            .map(|(&hash, path)| (hash, StackPosition::from_path_step(path)))
            .collect();

        DepthInfo {
            stack_depth: stack_hashes.len(),
            stack_hashes,
            stack_hash_set,
            stack_class_types,
            stack_positions,
        }
    }

    pub fn trim(&self, amount: usize) -> DepthInfo {
        // Return a DepthInfo object with some amount trimmed.
        if amount == 0 {
            // The trivial case.
            return self.clone();
        }

        let slice_set: IntSet<_> = IntSet::from_iter(
            self.stack_hashes[self.stack_hashes.len() - amount..].iter().copied(),
        );

        let new_hash_set: IntSet<_> = self.stack_hash_set.difference(&slice_set).cloned().collect();

        DepthInfo {
            stack_depth: self.stack_depth - amount,
            stack_hashes: self.stack_hashes[..self.stack_hashes.len() - amount].to_vec(),
            stack_hash_set: new_hash_set.clone(),
            stack_class_types: self.stack_class_types[..self.stack_class_types.len() - amount]
                .to_vec(),
            stack_positions: self
                .stack_positions
                .iter()
                .filter(|(k, _)| new_hash_set.contains(k))
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
        }
    }

    pub fn common_with(&self, other: &DepthInfo) -> Vec<u64> {
        // Get the common depth and hashes with the other.
        // We use AHashSet intersection because it's efficient and hashes should be
        // unique.

        let common_hashes: AHashSet<_> = self
            .stack_hash_set
            .intersection(&other.stack_hashes.iter().copied().collect())
            .cloned()
            .collect();

        // We should expect there to be _at least_ one common ancestor, because
        // they should share the same file segment. If that's not the case we
        // should error because it's likely a bug or programming error.
        assert!(!common_hashes.is_empty(), "DepthInfo comparison shares no common ancestor!");

        let common_depth = common_hashes.len();
        self.stack_hashes.iter().take(common_depth).cloned().collect()
    }
}

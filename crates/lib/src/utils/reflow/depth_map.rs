use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ahash::{AHashMap, AHashSet};
use uuid::Uuid;

use crate::core::parser::segments::base::{ErasedSegment, PathStep, Segment};

/// An element of the stack_positions property of DepthInfo.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StackPosition {
    pub idx: usize,
    pub len: usize,
    pub type_: String,
}

impl StackPosition {
    /// Interpret a path step for stack_positions.
    fn stack_pos_interpreter(path_step: &PathStep) -> String {
        if path_step.code_idxs.is_empty() {
            "".to_string()
        } else if path_step.code_idxs.len() == 1 {
            "solo".to_string()
        } else if path_step.idx == *path_step.code_idxs.iter().min().unwrap() {
            "start".to_string()
        } else if path_step.idx == *path_step.code_idxs.iter().max().unwrap() {
            "end".to_string()
        } else {
            "".to_string()
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
        let mut depth_info = AHashMap::new();

        for (raw, stack) in raws_with_stack {
            depth_info.insert(raw.get_uuid().unwrap(), DepthInfo::from_raw_and_stack(raw, stack));
        }

        Self { depth_info }
    }

    pub fn get_depth_info(&self, seg: &ErasedSegment) -> DepthInfo {
        self.depth_info[&seg.get_uuid().unwrap()].clone()
    }

    pub fn from_parent(parent: &dyn Segment) -> Self {
        Self::new(parent.raw_segments_with_ancestors())
    }

    pub fn from_raws_and_root(
        raw_segments: Vec<ErasedSegment>,
        root_segment: ErasedSegment,
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
    pub stack_hash_set: AHashSet<u64>,
    pub stack_class_types: Vec<AHashSet<String>>,
    pub stack_positions: AHashMap<u64, StackPosition>,
}

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    t.hash(&mut hasher);
    hasher.finish()
}

impl DepthInfo {
    fn from_raw_and_stack(raw: ErasedSegment, stack: Vec<PathStep>) -> DepthInfo {
        let stack_hashes: Vec<u64> = stack
            .iter()
            .map(|ps| {
                let hash = calculate_hash(&ps.segment);
                hash
            })
            .collect();

        let stack_hash_set: AHashSet<u64> = stack_hashes.iter().cloned().collect();

        let stack_class_types: Vec<AHashSet<String>> =
            stack.iter().map(|ps| ps.segment.class_types().iter().cloned().collect()).collect();

        let stack_positions: AHashMap<u64, StackPosition> = stack
            .into_iter()
            .map(|ps| {
                let hash = calculate_hash(&ps.segment);
                (hash, StackPosition::from_path_step(&ps))
            })
            .collect();

        DepthInfo {
            stack_depth: stack_hashes.len(),
            stack_hashes,
            stack_hash_set,
            stack_class_types,
            stack_positions,
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

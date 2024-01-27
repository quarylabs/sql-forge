use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use uuid::Uuid;

use crate::core::parser::segments::base::{PathStep, Segment};

/// An element of the stack_positions property of DepthInfo.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StackPosition {
    idx: usize,
    len: usize,
    type_: String,
}

impl StackPosition {
    /// Interpret a path step for stack_positions.
    fn stack_pos_interpreter(path_step: &PathStep) -> String {
        if path_step.idx == 0 && path_step.idx == path_step.len - 1 {
            "solo".to_string()
        } else if path_step.idx == 0 {
            "start".to_string()
        } else if path_step.idx == path_step.len - 1 {
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

pub struct DepthMap {
    depth_info: HashMap<Uuid, DepthInfo>,
}

impl DepthMap {
    fn new(raws_with_stack: Vec<(Box<dyn Segment>, Vec<PathStep>)>) -> Self {
        let mut depth_info = HashMap::new();

        for (raw, stack) in raws_with_stack {
            depth_info.insert(raw.get_uuid().unwrap(), DepthInfo::from_raw_and_stack(raw, stack));
        }

        Self { depth_info }
    }

    pub fn get_depth_info(&self, seg: &Box<dyn Segment>) -> DepthInfo {
        self.depth_info[&seg.get_uuid().unwrap()].clone()
    }

    pub fn from_raws_and_root(
        raw_segments: Vec<Box<dyn Segment>>,
        root_segment: Box<dyn Segment>,
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
#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct DepthInfo {
    pub stack_depth: usize,
    pub stack_hashes: Vec<u64>,
    /// This is a convenience cache to speed up operations.
    pub stack_hash_set: HashSet<u64>,
    pub stack_class_types: Vec<HashSet<String>>,
    pub stack_positions: HashMap<u64, StackPosition>,
}

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    t.hash(&mut hasher);
    hasher.finish()
}

impl DepthInfo {
    fn from_raw_and_stack(raw: Box<dyn Segment>, stack: Vec<PathStep>) -> DepthInfo {
        let stack_hashes: Vec<u64> = stack
            .iter()
            .map(|ps| {
                let hash = calculate_hash(&ps.segment);
                hash
            })
            .collect();

        let stack_hash_set: HashSet<u64> = stack_hashes.iter().cloned().collect();

        let stack_class_types: Vec<HashSet<String>> =
            stack.iter().map(|ps| ps.segment.class_types().iter().cloned().collect()).collect();

        let stack_positions: HashMap<u64, StackPosition> = stack
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
}

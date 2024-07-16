use std::borrow::Cow;
use std::sync::OnceLock;

use ahash::AHashSet;
use itertools::Itertools;
use uuid::Uuid;

use super::base::{pos_marker, ErasedSegment, Segment};
use crate::core::errors::SQLParseError;
use crate::core::parser::context::ParseContext;
use crate::core::parser::markers::PositionMarker;
use crate::core::parser::match_result::MatchResult;
use crate::core::parser::matchable::{next_matchable_cache_key, Matchable, MatchableCacheKey};
use crate::dialects::ansi::VecCell;
use crate::helpers::ToErasedSegment;

#[derive(Debug, Clone)]
pub struct BracketedSegment {
    raw: OnceLock<String>,
    pub segments: VecCell<ErasedSegment>,
    pub start_bracket: Vec<ErasedSegment>,
    pub end_bracket: Vec<ErasedSegment>,
    pub pos_marker: Option<PositionMarker>,
    pub uuid: Uuid,
    cache_key: MatchableCacheKey,
    descendant_type_set: OnceLock<AHashSet<&'static str>>,
}

impl PartialEq for BracketedSegment {
    fn eq(&self, other: &Self) -> bool {
        self.segments.iter().zip(&other.segments).all(|(lhs, rhs)| lhs.dyn_eq(rhs))
            && self.start_bracket == other.start_bracket
            && self.end_bracket == other.end_bracket
    }
}

impl BracketedSegment {
    pub fn new(
        segments: Vec<ErasedSegment>,
        start_bracket: Vec<ErasedSegment>,
        end_bracket: Vec<ErasedSegment>,
        hack: bool,
    ) -> Self {
        let mut this = BracketedSegment {
            segments: VecCell:,
            start_bracket,
            end_bracket,
            pos_marker: None,
            uuid: Uuid::new_v4(),
            raw: OnceLock::new(),
            cache_key: next_matchable_cache_key(),
            descendant_type_set: OnceLock::new(),
        };
        if !hack {
            this.pos_marker = pos_marker(&this.segments).into();
        }
        this
    }
}

impl Segment for BracketedSegment {
    fn new(&self, segments: Vec<ErasedSegment>) -> ErasedSegment {
        let mut this = self.clone();
        this.segments = segments;
        this.raw = OnceLock::new();
        this.pos_marker = pos_marker(&this.segments).into();
        this.to_erased_segment()
    }

    fn raw(&self) -> Cow<str> {
        self.raw.get_or_init(|| self.segments().iter().map(|segment| segment.raw()).join("")).into()
    }

    fn get_type(&self) -> &'static str {
        "bracketed"
    }

    fn get_position_marker(&self) -> Option<PositionMarker> {
        self.pos_marker.clone()
    }

    fn segments(&self) -> &[ErasedSegment] {
        &self.segments
    }

    fn set_segments(&self, segments: Vec<ErasedSegment>) {
        self.segments.swap(segments);
    }

    fn set_position_marker(&mut self, position_marker: Option<PositionMarker>) {
        self.pos_marker = position_marker;
    }

    fn get_uuid(&self) -> Uuid {
        self.uuid
    }

    fn class_types(&self) -> AHashSet<&'static str> {
        ["bracketed"].into()
    }

    fn descendant_type_set(&self) -> &AHashSet<&'static str> {
        self.descendant_type_set.get_or_init(|| {
            let mut result_set = AHashSet::new();

            for seg in self.segments() {
                result_set.extend(seg.descendant_type_set().union(&seg.class_types()));
            }

            result_set
        })
    }
}

impl Matchable for BracketedSegment {
    fn simple(
        &self,
        _parse_context: &ParseContext,
        _crumbs: Option<Vec<&str>>,
    ) -> Option<(AHashSet<String>, AHashSet<&'static str>)> {
        None
    }

    fn match_segments(
        &self,
        segments: &[ErasedSegment],
        idx: u32,
        _parse_context: &mut ParseContext,
    ) -> Result<MatchResult, SQLParseError> {
        if segments[idx as usize].as_any().downcast_ref::<BracketedSegment>().is_some() {
            return Ok(MatchResult::from_span(idx, idx + 1));
        }

        Ok(MatchResult::empty_at(idx))
    }

    fn cache_key(&self) -> MatchableCacheKey {
        self.cache_key
    }
}

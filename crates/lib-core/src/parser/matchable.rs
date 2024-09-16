use std::any::Any;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use ahash::AHashSet;
use dyn_ord::DynEq;

use super::context::ParseContext;
use super::grammar::base::Ref;
use super::match_result::MatchResult;
use super::segments::base::ErasedSegment;
use crate::dialects::syntax::{SyntaxKind, SyntaxSet};
use crate::errors::SQLParseError;

pub trait AsAnyMut {
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAnyMut for T {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub trait Matchable: Any + Debug + DynEq + AsAnyMut + Send + Sync + dyn_clone::DynClone {
    fn mk_from_segments(&self, segments: Vec<ErasedSegment>) -> ErasedSegment {
        let _ = segments;
        unimplemented!("{}", std::any::type_name::<Self>())
    }

    fn get_type(&self) -> SyntaxKind {
        todo!()
    }

    fn match_grammar(&self) -> Option<Arc<dyn Matchable>> {
        None
    }

    fn hack_eq(&self, rhs: &Arc<dyn Matchable>) -> bool {
        let lhs = self.as_any().downcast_ref::<Ref>();
        let rhs = rhs.as_any().downcast_ref::<Ref>();

        lhs.zip(rhs).map_or(false, |(lhs, rhs)| lhs.reference == rhs.reference)
    }

    // Return whether this element is optional.
    fn is_optional(&self) -> bool {
        false
    }

    // Try to obtain a simple response from the matcher.
    // Returns a tuple of two sets of strings if simple.
    // The first is a set of uppercase raw strings which would match.
    // The second is a set of segment types that would match.
    // Returns None if not simple.
    // Note: the crumbs argument is used to detect recursion.

    fn simple(
        &self,
        parse_context: &ParseContext,
        crumbs: Option<Vec<&str>>,
    ) -> Option<(AHashSet<String>, SyntaxSet)> {
        let match_grammar = self.match_grammar()?;

        match_grammar.simple(parse_context, crumbs)
    }

    // Match against this matcher.

    fn match_segments(
        &self,
        _segments: &[ErasedSegment],
        _idx: u32,
        _parse_context: &mut ParseContext,
    ) -> Result<MatchResult, SQLParseError> {
        todo!();
    }

    // A method to generate a unique cache key for the matchable object.
    //
    // Returns none for no caching key
    fn cache_key(&self) -> MatchableCacheKey {
        unimplemented!("{}", std::any::type_name::<Self>())
    }

    #[track_caller]
    fn copy(
        &self,
        _insert: Option<Vec<Arc<dyn Matchable>>>,
        _at: Option<usize>,
        _before: Option<Arc<dyn Matchable>>,
        _remove: Option<Vec<Arc<dyn Matchable>>>,
        _terminators: Vec<Arc<dyn Matchable>>,
        _replace_terminators: bool,
    ) -> Arc<dyn Matchable> {
        unimplemented!("{}", std::any::type_name::<Self>())
    }
}

dyn_clone::clone_trait_object!(Matchable);

pub type MatchableCacheKey = u32;

pub fn next_matchable_cache_key() -> MatchableCacheKey {
    // The value 0 is reserved for NonCodeMatcher. This grammar matcher is somewhat
    // of a singleton, so we don't need a unique ID in the same way as other grammar
    // matchers.
    static ID: AtomicU32 = AtomicU32::new(1);

    ID.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1)).unwrap()
}

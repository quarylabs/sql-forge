use std::ops::Range;

use ahash::AHashSet;

use crate::edit_type::EditType;
use crate::parser::segments::base::ErasedSegment;
use crate::templaters::base::{RawFileSlice, TemplatedFile};

/// A class to hold a potential fix to a linting violation.
///
///     Args:
///         edit_type (:obj:`str`): One of `create_before`, `create_after`,
///             `replace`, `delete` to indicate the kind of fix this represents.
///         anchor (:obj:`BaseSegment`): A segment which represents
///             the *position* that this fix should be applied at. For deletions
///             it represents the segment to delete, for creations it implies
/// the             position to create at (with the existing element at this
/// position             to be moved *after* the edit), for a `replace` it
/// implies the             segment to be replaced.
///         edit (iterable of :obj:`BaseSegment`, optional): For `replace` and
///             `create` fixes, this holds the iterable of segments to create
///             or replace at the given `anchor` point.
///         source (iterable of :obj:`BaseSegment`, optional): For `replace` and
///             `create` fixes, this holds iterable of segments that provided
///             code. IMPORTANT: The linter uses this to prevent copying
/// material             from templated areas.
#[derive(Debug, Clone)]
pub struct LintFix {
    pub edit_type: EditType,
    pub anchor: ErasedSegment,
    pub edit: Option<Vec<ErasedSegment>>,
    pub source: Vec<ErasedSegment>,
}

impl LintFix {
    fn new(
        edit_type: EditType,
        anchor: ErasedSegment,
        edit: Option<Vec<ErasedSegment>>,
        source: Option<Vec<ErasedSegment>>,
    ) -> Self {
        // If `edit` is provided, copy all elements and strip position markers.
        let mut clean_edit = None;
        if let Some(mut edit) = edit {
            // Developer Note: Ensure position markers are unset for all edit segments.
            // We rely on realignment to make position markers later in the process.
            for seg in &mut edit {
                if seg.get_position_marker().is_some() {
                    seg.make_mut().set_position_marker(None);
                };
            }
            clean_edit = Some(edit);
        }

        // If `source` is provided, filter segments with position markers.
        let clean_source = source.map_or(Vec::new(), |source| {
            source.into_iter().filter(|seg| seg.get_position_marker().is_some()).collect()
        });

        LintFix { edit_type, anchor, edit: clean_edit, source: clean_source }
    }

    pub fn create_before(anchor: ErasedSegment, edit_segments: Vec<ErasedSegment>) -> Self {
        Self::new(EditType::CreateBefore, anchor, edit_segments.into(), None)
    }

    pub fn create_after(
        anchor: ErasedSegment,
        edit_segments: Vec<ErasedSegment>,
        source: Option<Vec<ErasedSegment>>,
    ) -> Self {
        Self::new(EditType::CreateAfter, anchor, edit_segments.into(), source)
    }

    pub fn replace(
        anchor_segment: ErasedSegment,
        edit_segments: Vec<ErasedSegment>,
        source: Option<Vec<ErasedSegment>>,
    ) -> Self {
        Self::new(EditType::Replace, anchor_segment, Some(edit_segments), source)
    }

    pub fn delete(anchor_segment: ErasedSegment) -> Self {
        Self::new(EditType::Delete, anchor_segment, None, None)
    }

    /// Return whether this a valid source only edit.
    pub fn is_just_source_edit(&self) -> bool {
        if let Some(edit) = &self.edit {
            self.edit_type == EditType::Replace
                && edit.len() == 1
                && edit[0].raw() == self.anchor.raw()
        } else {
            false
        }
    }

    fn fix_slices(
        &self,
        templated_file: &TemplatedFile,
        within_only: bool,
    ) -> AHashSet<RawFileSlice> {
        let anchor_slice = self.anchor.get_position_marker().unwrap().templated_slice.clone();

        let adjust_boundary = if !within_only { 1 } else { 0 };

        let templated_slice = match self.edit_type {
            EditType::CreateBefore => anchor_slice.start - 1..anchor_slice.start + adjust_boundary,
            EditType::CreateAfter => anchor_slice.end - adjust_boundary..anchor_slice.end + 1,
            EditType::Replace => {
                let pos = self.anchor.get_position_marker().unwrap();
                if pos.source_slice.start == pos.source_slice.end {
                    return AHashSet::new();
                } else if self
                    .edit
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .all(|it| it.segments().is_empty() && !it.get_source_fixes().is_empty())
                {
                    let source_edit_slices: Vec<_> = self
                        .edit
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .flat_map(|edit| edit.get_source_fixes())
                        .map(|source_fixe| source_fixe.source_slice.clone())
                        .collect();

                    let slice =
                        templated_file.raw_slices_spanning_source_slice(&source_edit_slices[0]);
                    return AHashSet::from_iter(slice);
                }

                anchor_slice
            }
            _ => anchor_slice,
        };

        self.raw_slices_from_templated_slices(
            templated_file,
            std::iter::once(templated_slice),
            RawFileSlice::new(String::new(), "literal".to_string(), usize::MAX, None, None).into(),
        )
    }

    fn raw_slices_from_templated_slices(
        &self,
        templated_file: &TemplatedFile,
        templated_slices: impl Iterator<Item = Range<usize>>,
        file_end_slice: Option<RawFileSlice>,
    ) -> AHashSet<RawFileSlice> {
        let mut raw_slices = AHashSet::new();

        for templated_slice in templated_slices {
            let templated_slice =
                templated_file.templated_slice_to_source_slice(templated_slice.clone());

            match templated_slice {
                Ok(templated_slice) => raw_slices
                    .extend(templated_file.raw_slices_spanning_source_slice(&templated_slice)),
                Err(_) => {
                    if let Some(file_end_slice) = file_end_slice.clone() {
                        raw_slices.insert(file_end_slice);
                    }
                }
            }
        }

        raw_slices
    }

    pub fn has_template_conflicts(&self, templated_file: &TemplatedFile) -> bool {
        if self.edit_type == EditType::Replace
            && self.edit.is_none()
            && self.edit.as_ref().unwrap().len() == 1
        {
            let edit = &self.edit.as_ref().unwrap()[0];
            if edit.raw() == self.anchor.raw() && !edit.get_source_fixes().is_empty() {
                return false;
            }
        }

        let check_fn = if let EditType::CreateAfter | EditType::CreateBefore = self.edit_type {
            itertools::all
        } else {
            itertools::any
        };

        let fix_slices = self.fix_slices(templated_file, false);
        let result = check_fn(fix_slices, |fs: RawFileSlice| fs.slice_type == "templated");

        if result || self.source.is_empty() {
            return result;
        }

        let templated_slices = None;
        let raw_slices = self.raw_slices_from_templated_slices(
            templated_file,
            templated_slices.into_iter(),
            None,
        );
        raw_slices.iter().any(|fs| fs.slice_type == "templated")
    }
}

impl PartialEq for LintFix {
    fn eq(&self, other: &Self) -> bool {
        // Check if edit_types are equal
        if self.edit_type != other.edit_type {
            return false;
        }
        // Check if anchor.class_types are equal
        if self.anchor.get_type() != other.anchor.get_type() {
            return false;
        }
        // Check if anchor.uuids are equal
        if self.anchor.id() != other.anchor.id() {
            return false;
        }
        // Compare edits if they exist
        if let Some(self_edit) = &self.edit {
            if let Some(other_edit) = &other.edit {
                // Check lengths
                if self_edit.len() != other_edit.len() {
                    return false;
                }
                // Compare raw and source_fixes for each corresponding BaseSegment
                for (self_base_segment, other_base_segment) in self_edit.iter().zip(other_edit) {
                    if self_base_segment.raw() != other_base_segment.raw()
                        || self_base_segment.get_source_fixes()
                            != other_base_segment.get_source_fixes()
                    {
                        return false;
                    }
                }
            } else {
                // self has edit, other doesn't
                return false;
            }
        } else if other.edit.is_some() {
            // other has edit, self doesn't
            return false;
        }
        // If none of the above conditions were met, objects are equal
        true
    }
}

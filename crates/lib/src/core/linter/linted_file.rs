use std::ops::Range;

use ahash::AHashSet;
use itertools::Itertools;

use crate::core::errors::{SQLBaseError, SQLLintError};
use crate::core::parser::segments::base::ErasedSegment;
use crate::core::parser::segments::fix::FixPatch;
use crate::core::templaters::base::{RawFileSlice, TemplatedFile};

#[derive(Debug)]
pub struct LintedFile {
    pub path: String,
    pub tree: ErasedSegment,
    pub templated_file: TemplatedFile,
    pub violations: Vec<SQLLintError>,
}

impl LintedFile {
    #[allow(unused_variables)]
    pub fn get_violations(&self, fixable: Option<bool>) -> Vec<SQLBaseError> {
        self.violations.clone().into_iter().map(Into::into).collect_vec()
    }

    ///  Use patches and raw file to fix the source file.
    ///
    ///  This assumes that patches and slices have already
    ///  been coordinated. If they haven't then this will
    ///  fail because we rely on patches having a corresponding
    ///  slice of exactly the right file in the list of file
    ///  slices.
    pub fn build_up_fixed_source_string(
        source_file_slices: &[Range<usize>],
        source_patches: &[FixPatch],
        raw_source_string: &str,
    ) -> String {
        // Iterate through the patches, building up the new string.
        let mut str_buff = String::new();
        for source_slice in source_file_slices.iter() {
            // Is it one in the patch buffer:
            let mut is_patched = false;
            for patch in source_patches.iter() {
                if patch.source_slice == *source_slice {
                    str_buff.push_str(&patch.fixed_raw);
                    is_patched = true;
                    break;
                }
            }
            if !is_patched {
                // Use the raw string
                str_buff.push_str(&raw_source_string[source_slice.start..source_slice.end]);
            }
        }
        str_buff
    }

    pub fn fix_string(&self) -> String {
        // Generate patches from the fixed tree. In the process we sort
        // and deduplicate them so that the resultant list is in the
        // the right order for the source file without any duplicates.
        let filtered_source_patches =
            Self::generate_source_patches(self.tree.clone(), &self.templated_file);

        // Any Template tags in the source file are off limits, unless we're explicitly
        // fixing the source file.
        let source_only_slices = self.templated_file.source_only_slices();

        // We now slice up the file using the patches and any source only slices.
        // This gives us regions to apply changes to.
        let slice_buff = Self::slice_source_file_using_patches(
            filtered_source_patches.clone(),
            source_only_slices,
            &self.templated_file.source_str,
        );

        Self::build_up_fixed_source_string(
            &slice_buff,
            &filtered_source_patches,
            &self.templated_file.source_str,
        )
    }

    #[allow(unused_variables)]
    fn generate_source_patches(
        tree: ErasedSegment,
        templated_file: &TemplatedFile,
    ) -> Vec<FixPatch> {
        // Placeholder for logger setup or integration
        let mut filtered_source_patches = Vec::new();
        let mut dedupe_buffer = AHashSet::new();

        for (idx, patch) in tree.iter_patches(templated_file).into_iter().enumerate() {
            // Log patch details
            // Logger::debug(format!("{} Yielded patch: {:?}", idx, patch));

            // Check for duplicates
            if dedupe_buffer.contains(&patch.dedupe_tuple()) {
                // Logger::info(format!("Skipping. Source space Duplicate: {:?}",
                // patch.dedupe_tuple()));
                continue;
            }

            // Implement logic to process patches
            // This involves translating the logic from Python to Rust, considering how
            // patches are evaluated, and how they interact with templated
            // sections.

            // Add valid patch to filtered list and dedupe buffer
            filtered_source_patches.push(patch.clone());
            dedupe_buffer.insert(patch.dedupe_tuple());
        }

        // Sort patches before returning
        filtered_source_patches.sort_by_key(|x| x.source_slice.start);
        filtered_source_patches
    }

    ///  Use patches to safely slice up the file before fixing.
    ///
    ///  This uses source only slices to avoid overwriting sections
    ///  of templated code in the source file (when we don't want to).
    ///
    ///  We assume that the source patches have already been
    ///  sorted and deduplicated. Sorting is important. If the slices
    ///  aren't sorted then this function will miss chunks.
    ///  If there are overlaps or duplicates then this function
    ///  may produce strange results.
    fn slice_source_file_using_patches(
        source_patches: Vec<FixPatch>,
        source_only_slices: Vec<RawFileSlice>,
        raw_source_string: &str,
    ) -> Vec<Range<usize>> {
        // We now slice up the file using the patches and any source only slices.
        // This gives us regions to apply changes to.
        let mut slice_buff: Vec<Range<usize>> = Vec::new();
        let mut source_idx = 0;
        let mut source_only_slices = source_only_slices.clone();

        for patch in &source_patches {
            // Are there templated slices at or before the start of this patch?
            // TODO: We'll need to explicit handling for template fixes here, because
            // they ARE source only slices. If we can get handling to work properly
            // here then this is the last hurdle and it will flow through
            // smoothly from here.
            while source_only_slices
                .first()
                .map_or(false, |s| s.source_idx < patch.source_slice.start)
            {
                let next_so_slice = source_only_slices.remove(0).source_slice();
                // Add a pre-slice before the next templated slices if needed.
                if next_so_slice.end > source_idx {
                    slice_buff.push(source_idx..next_so_slice.start);
                }
                // Add the templated slice.
                slice_buff.push(next_so_slice.clone());
                source_idx = next_so_slice.end;
            }

            // Does this patch cover the next source-only slice directly?
            if source_only_slices.first().map_or(false, |s| patch.source_slice == s.source_slice())
            {
                // Log information here if needed
                // Removing next source only slice from the stack because it
                // covers the same area of source file as the current patch.
                source_only_slices.remove(0);
            }

            // Is there a gap between current position and this patch?
            if patch.source_slice.start > source_idx {
                // Add a slice up to this patch.
                slice_buff.push(source_idx..patch.source_slice.start);
            }

            // Is this patch covering an area we've already covered?
            if patch.source_slice.start < source_idx {
                // NOTE: This shouldn't happen. With more detailed templating
                // this shouldn't happen - but in the off-chance that this does
                // happen - then this code path remains.
                // Log information here if needed
                // Skipping overlapping patch at Index.
                continue;
            }

            // Add this patch.
            slice_buff.push(patch.source_slice.clone());
            source_idx = patch.source_slice.end;
        }
        // Add a tail slice.
        if source_idx < raw_source_string.len() {
            slice_buff.push(source_idx..raw_source_string.len());
        }

        slice_buff
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::core::templaters::base::TemplatedFileSlice;

    /// Test _build_up_fixed_source_string. This is part of fix_string().
    #[test]
    fn test__linted_file__build_up_fixed_source_string() {
        let tests = [
            // Trivial example
            (vec![0..1], vec![], "a", "a"),
            // Simple replacement
            (
                vec![0..1, 1..2, 2..3],
                vec![FixPatch::new(
                    1..2,
                    "d".to_string(),
                    "".to_string(),
                    1..2,
                    "b".to_string(),
                    "b".to_string(),
                )],
                "abc",
                "adc",
            ),
            // Simple insertion
            (
                vec![0..1, 1..1, 1..2],
                vec![FixPatch::new(
                    1..1,
                    "b".to_string(),
                    "".to_string(),
                    1..1,
                    "".to_string(),
                    "".to_string(),
                )],
                "ac",
                "abc",
            ),
            // Simple deletion
            (
                vec![0..1, 1..2, 2..3],
                vec![FixPatch::new(
                    1..2,
                    "".to_string(),
                    "".to_string(),
                    1..2,
                    "b".to_string(),
                    "b".to_string(),
                )],
                "abc",
                "ac",
            ),
            // Illustrative templated example (although practically at this step, the routine
            // shouldn't care if it's templated).
            (
                vec![0..2, 2..7, 7..9],
                vec![FixPatch::new(
                    2..3,
                    "{{ b }}".to_string(),
                    "".to_string(),
                    2..7,
                    "b".to_string(),
                    "{{b}}".to_string(),
                )],
                "a {{b}} c",
                "a {{ b }} c",
            ),
        ];

        for (source_file_slices, source_patches, raw_source_string, expected_result) in tests {
            let result = LintedFile::build_up_fixed_source_string(
                &source_file_slices,
                &source_patches,
                raw_source_string,
            );

            assert_eq!(result, expected_result)
        }
    }

    /// Test _slice_source_file_using_patches.
    ///
    ///     This is part of fix_string().
    #[test]
    fn test__slice_source_file_using_patches() {
        #[allow(clippy::single_range_in_vec_init)]
        let test_cases = [
            (
                // Trivial example.
                // No edits in a single character file. Slice should be one
                // character long.
                vec![],
                vec![],
                "a",
                vec![0..1],
            ),
            (
                // Simple replacement.
                // We've yielded a patch to change a single character. This means
                // we should get only slices for that character, and for the
                // unchanged file around it.
                vec![FixPatch::new(
                    1..2,
                    "d".to_string(),
                    "".to_string(),
                    1..2,
                    "b".to_string(),
                    "b".to_string(),
                )],
                vec![],
                "abc",
                vec![0..1, 1..2, 2..3],
            ),
            (
                // Templated no fixes.
                // A templated file, but with no fixes, so no subdivision of the
                // file is required and we should just get a single slice.
                vec![],
                vec![],
                "a {{ b }} c",
                vec![0..11],
            ),
            (
                // Templated example with a source-only slice.
                // A templated file, but with no fixes, so no subdivision of the
                // file is required and we should just get a single slice. While
                // there is handling for "source only" slices like template
                // comments, in this case no additional slicing is required
                // because no edits have been made.
                vec![],
                vec![RawFileSlice::new(
                    "{# b #}".to_string(),
                    "comment".to_string(),
                    2,
                    None,
                    None,
                )],
                "a {# b #} c",
                vec![0..11],
            ),
            (
                // Templated fix example with a source-only slice.
                // We're making an edit adjacent to a source only slice. Edits
                // _before_ source only slices currently don't trigger additional
                // slicing. This is fine.
                vec![FixPatch::new(
                    0..1,
                    "a ".to_string(),
                    "".to_string(),
                    0..1,
                    "a".to_string(),
                    "a".to_string(),
                )],
                vec![RawFileSlice::new(
                    "{# b #}".to_string(),
                    "comment".to_string(),
                    1,
                    None,
                    None,
                )],
                "a{# b #}c",
                vec![0..1, 1..9],
            ),
            (
                // Templated fix example with a source-only slice.
                // We've made an edit directly _after_ a source only slice
                // which should trigger the logic to ensure that the source
                // only slice isn't included in the source mapping of the
                // edit.
                vec![FixPatch::new(
                    1..2,
                    " c".to_string(),
                    "".to_string(),
                    8..9,
                    "c".to_string(),
                    "c".to_string(),
                )],
                vec![RawFileSlice::new(
                    "{# b #}".to_string(),
                    "comment".to_string(),
                    1,
                    None,
                    None,
                )],
                "a{# b #}cc",
                vec![0..1, 1..8, 8..9, 9..10],
            ),
            (
                // Templated example with a source-only slice.
                // Here we're making the fix to the templated slice. This
                // checks that we don't duplicate or fumble the slice
                // generation when we're explicitly trying to edit the source.
                vec![FixPatch::new(
                    2..2,
                    "{# fixed #}".to_string(),
                    "".to_string(),
                    2..9,
                    "".to_string(),
                    "".to_string(),
                )],
                vec![RawFileSlice::new(
                    "{# b #}".to_string(),
                    "comment".to_string(),
                    2,
                    None,
                    None,
                )],
                "a {# b #} c",
                vec![0..2, 2..9, 9..11],
            ),
            (
                // Illustrate potential templating bug (case from JJ01).
                // In this case we have fixes for all our tempolated sections
                // and they are all close to each other and so may be either
                // skipped or duplicated if the logic is not precise.
                vec![
                    FixPatch::new(
                        14..14,
                        "{%+ if true -%}".to_string(),
                        "source".to_string(),
                        14..27,
                        "".to_string(),
                        "{%+if true-%}".to_string(),
                    ),
                    FixPatch::new(
                        14..14,
                        "{{ ref('foo') }}".to_string(),
                        "source".to_string(),
                        28..42,
                        "".to_string(),
                        "{{ref('foo')}}".to_string(),
                    ),
                    FixPatch::new(
                        17..17,
                        "{%- endif %}".to_string(),
                        "source".to_string(),
                        43..53,
                        "".to_string(),
                        "{%-endif%}".to_string(),
                    ),
                ],
                vec![
                    RawFileSlice::new(
                        "{%+if true-%}".to_string(),
                        "block_start".to_string(),
                        14,
                        None,
                        None,
                    ),
                    RawFileSlice::new(
                        "{%-endif%}".to_string(),
                        "block_end".to_string(),
                        43,
                        None,
                        None,
                    ),
                ],
                "SELECT 1 from {%+if true-%} {{ref('foo')}} {%-endif%}",
                vec![0..14, 14..27, 27..28, 28..42, 42..43, 43..53],
            ),
        ];

        for (source_patches, source_only_slices, raw_source_string, expected_result) in test_cases {
            let result = LintedFile::slice_source_file_using_patches(
                source_patches,
                source_only_slices,
                raw_source_string,
            );
            assert_eq!(result, expected_result);
        }
    }

    #[allow(dead_code)]
    fn templated_file_1() -> TemplatedFile {
        TemplatedFile::from_string("abc".to_string())
    }

    #[allow(dead_code)]
    fn templated_file_2() -> TemplatedFile {
        TemplatedFile::new(
            "{# blah #}{{ foo }}bc".to_string(),
            "<testing>".to_string(),
            Some("abc".to_string()),
            Some(vec![
                TemplatedFileSlice::new("comment", 0..10, 0..0),
                TemplatedFileSlice::new("templated", 10..19, 0..1),
                TemplatedFileSlice::new("literal", 19..21, 1..3),
            ]),
            Some(vec![
                RawFileSlice::new("{# blah #}".to_string(), "comment".to_string(), 0, None, None),
                RawFileSlice::new("{{ foo }}".to_string(), "templated".to_string(), 10, None, None),
                RawFileSlice::new("bc".to_string(), "literal".to_string(), 19, None, None),
            ]),
        )
        .unwrap()
    }
}

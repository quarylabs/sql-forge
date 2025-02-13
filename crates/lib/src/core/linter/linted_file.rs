use std::ops::Range;

use crate::core::rules::noqa::IgnoreMask;
use itertools::Itertools;
use rustc_hash::FxHashSet;
use sqruff_lib_core::errors::{SQLBaseError, SqlError};
use sqruff_lib_core::parser::segments::fix::FixPatch;
use sqruff_lib_core::templaters::base::{RawFileSlice, TemplatedFile};

#[derive(Debug, Default, Clone)]
pub struct LintedFile {
    pub path: String,
    pub patches: Vec<FixPatch>,
    pub templated_file: TemplatedFile,
    pub violations: Vec<SQLBaseError>,
    pub ignore_mask: Option<IgnoreMask>,
}

impl LintedFile {
    pub fn get_violations(&self, is_fixable: Option<bool>) -> Vec<SQLBaseError> {
        if let Some(is_fixable) = is_fixable {
            self.violations
                .iter()
                .filter(|v| v.fixable() == is_fixable)
                .cloned()
                .collect_vec()
        } else {
            self.violations.clone().into_iter().map_into().collect_vec()
        }
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

    pub fn fix_string(self) -> String {
        // Generate patches from the fixed tree. In the process we sort
        // and deduplicate them so that the resultant list is in the
        //  right order for the source file without any duplicates.
        let filtered_source_patches =
            Self::generate_source_patches(self.patches, &self.templated_file);

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

    fn generate_source_patches(
        patches: Vec<FixPatch>,
        _templated_file: &TemplatedFile,
    ) -> Vec<FixPatch> {
        let mut filtered_source_patches = Vec::new();
        let mut dedupe_buffer = FxHashSet::default();

        for patch in patches {
            if dedupe_buffer.insert(patch.dedupe_tuple()) {
                filtered_source_patches.push(patch);
            }
        }

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
        mut source_only_slices: Vec<RawFileSlice>,
        raw_source_string: &str,
    ) -> Vec<Range<usize>> {
        // We now slice up the file using the patches and any source only slices.
        // This gives us regions to apply changes to.
        let mut slice_buff: Vec<Range<usize>> = Vec::new();
        let mut source_idx = 0;

        for patch in &source_patches {
            // Are there templated slices at or before the start of this patch?
            // TODO: We'll need to explicit handling for template fixes here, because
            // they ARE source only slices. If we can get handling to work properly
            // here then this is the last hurdle and it will flow through
            // smoothly from here.
            while source_only_slices
                .first()
                .is_some_and(|s| s.source_idx < patch.source_slice.start)
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
            if source_only_slices
                .first()
                .is_some_and(|s| patch.source_slice == s.source_slice())
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
    use sqruff_lib_core::templaters::base::TemplatedFileSlice;

    use super::*;

    /// Test _build_up_fixed_source_string. This is part of fix_string().
    #[test]
    fn test_linted_file_build_up_fixed_source_string() {
        let tests = [
            // Trivial example
            (vec![0..1], vec![], "a", "a"),
            // Simple replacement
            (
                vec![0..1, 1..2, 2..3],
                vec![FixPatch::new(
                    1..2,
                    "d".into(),
                    1..2,
                    "b".into(),
                    "b".into(),
                )],
                "abc",
                "adc",
            ),
            // Simple insertion
            (
                vec![0..1, 1..1, 1..2],
                vec![FixPatch::new(1..1, "b".into(), 1..1, "".into(), "".into())],
                "ac",
                "abc",
            ),
            // Simple deletion
            (
                vec![0..1, 1..2, 2..3],
                vec![FixPatch::new(1..2, "".into(), 1..2, "b".into(), "b".into())],
                "abc",
                "ac",
            ),
            // Illustrative templated example (although practically at this step, the routine
            // shouldn't care if it's templated).
            (
                vec![0..2, 2..7, 7..9],
                vec![FixPatch::new(
                    2..3,
                    "{{ b }}".into(),
                    2..7,
                    "b".into(),
                    "{{b}}".into(),
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
    fn test_slice_source_file_using_patches() {
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
                    "d".into(),
                    1..2,
                    "b".into(),
                    "b".into(),
                )],
                vec![],
                "abc",
                vec![0..1, 1..2, 2..3],
            ),
            (
                // Templated no fixes.
                // A templated file, but with no fixes, so no subdivision of the
                // file is required, and we should just get a single slice.
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
                    "{# b #}".into(),
                    "comment".into(),
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
                    "a ".into(),
                    0..1,
                    "a".into(),
                    "a".into(),
                )],
                vec![RawFileSlice::new(
                    "{# b #}".into(),
                    "comment".into(),
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
                    " c".into(),
                    8..9,
                    "c".into(),
                    "c".into(),
                )],
                vec![RawFileSlice::new(
                    "{# b #}".into(),
                    "comment".into(),
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
                    "{# fixed #}".into(),
                    // "".into(),
                    2..9,
                    "".into(),
                    "".into(),
                )],
                vec![RawFileSlice::new(
                    "{# b #}".into(),
                    "comment".into(),
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
                        "{%+ if true -%}".into(),
                        // "source".into(),
                        14..27,
                        "".into(),
                        "{%+if true-%}".into(),
                    ),
                    FixPatch::new(
                        14..14,
                        "{{ ref('foo') }}".into(),
                        // "source".into(),
                        28..42,
                        "".into(),
                        "{{ref('foo')}}".into(),
                    ),
                    FixPatch::new(
                        17..17,
                        "{%- endif %}".into(),
                        // "source".into(),
                        43..53,
                        "".into(),
                        "{%-endif%}".into(),
                    ),
                ],
                vec![
                    RawFileSlice::new("{%+if true-%}".into(), "block_start".into(), 14, None, None),
                    RawFileSlice::new("{%-endif%}".into(), "block_end".into(), 43, None, None),
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
        "abc".into()
    }

    #[allow(dead_code)]
    fn templated_file_2() -> TemplatedFile {
        TemplatedFile::new(
            "{# blah #}{{ foo }}bc".into(),
            "<testing>".into(),
            Some("abc".into()),
            Some(vec![
                TemplatedFileSlice::new("comment", 0..10, 0..0),
                TemplatedFileSlice::new("templated", 10..19, 0..1),
                TemplatedFileSlice::new("literal", 19..21, 1..3),
            ]),
            Some(vec![
                RawFileSlice::new("{# blah #}".into(), "comment".into(), 0, None, None),
                RawFileSlice::new("{{ foo }}".into(), "templated".into(), 10, None, None),
                RawFileSlice::new("bc".into(), "literal".into(), 19, None, None),
            ]),
        )
        .unwrap()
    }
}

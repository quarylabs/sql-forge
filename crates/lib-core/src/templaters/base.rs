use std::cmp::Ordering;
use std::ops::{Deref, Range};
use std::sync::Arc;

use smol_str::SmolStr;

use crate::errors::{SQLFluffSkipFile, ValueError};
use crate::slice_helpers::zero_slice;

/// A slice referring to a templated file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TemplatedFileSlice {
    pub slice_type: String,
    pub source_slice: Range<usize>,
    pub templated_slice: Range<usize>,
}

impl TemplatedFileSlice {
    pub fn new(
        slice_type: &str,
        source_slice: Range<usize>,
        templated_slice: Range<usize>,
    ) -> Self {
        Self {
            slice_type: slice_type.to_string(),
            source_slice,
            templated_slice,
        }
    }
}

/// A templated SQL file.
///
/// This is the response of a `templater`'s `.process()` method
/// and contains both references to the original file and also
/// the capability to split up that file when lexing.
#[derive(Debug, PartialEq, Eq, Clone, Hash, Default)]
pub struct TemplatedFile {
    inner: Arc<TemplatedFileInner>,
}

impl TemplatedFile {
    pub fn new(
        source_str: String,
        f_name: String,
        input_templated_str: Option<String>,
        sliced_file: Option<Vec<TemplatedFileSlice>>,
        input_raw_sliced: Option<Vec<RawFileSlice>>,
    ) -> Result<TemplatedFile, SQLFluffSkipFile> {
        Ok(TemplatedFile {
            inner: Arc::new(TemplatedFileInner::new(
                source_str,
                f_name,
                input_templated_str,
                sliced_file,
                input_raw_sliced,
            )?),
        })
    }
}

impl From<String> for TemplatedFile {
    fn from(raw: String) -> Self {
        TemplatedFile {
            inner: Arc::new(
                TemplatedFileInner::new(raw, "<string>".to_string(), None, None, None).unwrap(),
            ),
        }
    }
}

impl From<&str> for TemplatedFile {
    fn from(raw: &str) -> Self {
        TemplatedFile {
            inner: Arc::new(
                TemplatedFileInner::new(raw.to_string(), "<string>".to_string(), None, None, None)
                    .unwrap(),
            ),
        }
    }
}

impl Deref for TemplatedFile {
    type Target = TemplatedFileInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Hash, Default)]
pub struct TemplatedFileInner {
    pub source_str: String,
    f_name: String,
    pub templated_str: Option<String>,
    source_newlines: Vec<usize>,
    templated_newlines: Vec<usize>,
    raw_sliced: Vec<RawFileSlice>,
    pub sliced_file: Vec<TemplatedFileSlice>,
}

impl TemplatedFileInner {
    /// Initialise the TemplatedFile.
    /// If no templated_str is provided then we assume that
    /// the file is NOT templated and that the templated view
    /// is the same as the source view.
    pub fn new(
        source_str: String,
        f_name: String,
        input_templated_str: Option<String>,
        sliced_file: Option<Vec<TemplatedFileSlice>>,
        input_raw_sliced: Option<Vec<RawFileSlice>>,
    ) -> Result<TemplatedFileInner, SQLFluffSkipFile> {
        // Assume that no sliced_file, means the file is not templated.
        // TODO Will this not always be Some and so type can avoid Option?
        let templated_str = input_templated_str.clone().unwrap_or(source_str.clone());

        let (sliced_file, raw_sliced): (Vec<TemplatedFileSlice>, Vec<RawFileSlice>) =
            match sliced_file {
                None => {
                    if templated_str != source_str {
                        panic!("Cannot instantiate a templated file unsliced!")
                    } else if input_raw_sliced.is_some() {
                        panic!("Templated file was not sliced, but not has raw slices.")
                    } else {
                        (
                            vec![TemplatedFileSlice::new(
                                "literal",
                                0..source_str.len(),
                                0..source_str.len(),
                            )],
                            vec![RawFileSlice::new(
                                source_str.clone(),
                                "literal".to_string(),
                                0,
                                None,
                                None,
                            )],
                        )
                    }
                }
                Some(sliced_file) => {
                    if let Some(raw_sliced) = input_raw_sliced {
                        (sliced_file, raw_sliced)
                    } else {
                        panic!("Templated file was sliced, but not raw.")
                    }
                }
            };

        // Precalculate newlines, character positions.
        let source_newlines: Vec<usize> = iter_indices_of_newlines(source_str.as_str()).collect();
        let templated_newlines: Vec<usize> =
            iter_indices_of_newlines(templated_str.as_str()).collect();

        // Consistency check raw string and slices.
        let mut pos = 0;
        for rfs in &raw_sliced {
            if rfs.source_idx != pos {
                panic!(
                    "TemplatedFile. Consistency fail on running source length. {} != {}",
                    pos, rfs.source_idx
                )
            }
            pos += rfs.raw.len();
        }
        if pos != source_str.len() {
            panic!(
                "TemplatedFile. Consistency fail on final source length. {} != {}",
                pos,
                source_str.len()
            )
        }

        // Consistency check templated string and slices.
        let mut previous_slice: Option<&TemplatedFileSlice> = None;
        let mut outer_tfs: Option<&TemplatedFileSlice> = None;
        for tfs in &sliced_file {
            match &previous_slice {
                Some(previous_slice) => {
                    if tfs.templated_slice.start != previous_slice.templated_slice.end {
                        return Err(SQLFluffSkipFile::new(
                            "Templated slices found to be non-contiguous.".to_string(),
                        ));
                        // TODO Make this nicer again
                        // format!(
                        //     "Templated slices found to be non-contiguous.
                        // {:?} (starting {:?}) does not follow {:?} (starting
                        // {:?})",
                        //     tfs.templated_slice,
                        //     templated_str[tfs.templated_slice],
                        //     previous_slice.templated_slice,
                        //     templated_str[previous_slice.templated_slice],
                        // )
                    }
                }
                None => {
                    if tfs.templated_slice.start != 0 {
                        return Err(SQLFluffSkipFile::new(format!(
                            "First templated slice does not start at 0, (found slice {:?})",
                            tfs.templated_slice
                        )));
                    }
                }
            }
            previous_slice = Some(tfs);
            outer_tfs = Some(tfs)
        }
        if !sliced_file.is_empty() && input_templated_str.is_some() {
            if let Some(outer_tfs) = outer_tfs {
                if outer_tfs.templated_slice.end != templated_str.len() {
                    return Err(SQLFluffSkipFile::new(format!(
                        "Last templated slice does not end at end of string, (found slice {:?})",
                        outer_tfs.templated_slice
                    )));
                }
            }
        }

        Ok(TemplatedFileInner {
            raw_sliced,
            source_newlines,
            templated_newlines,
            source_str: source_str.clone(),
            sliced_file,
            f_name,
            templated_str: Some(templated_str),
        })
    }

    /// Return true if there's a templated file.
    pub fn is_templated(&self) -> bool {
        self.templated_str.is_some()
    }

    pub fn raw_sliced_iter(&self) -> impl Iterator<Item = &RawFileSlice> {
        self.raw_sliced.iter()
    }

    /// Get the line number and position of a point in the source file.
    /// Args:
    ///  - char_pos: The character position in the relevant file.
    ///  - source: Are we checking the source file (as opposed to the templated
    ///    file)
    ///
    /// Returns: line_number, line_position
    pub fn get_line_pos_of_char_pos(&self, char_pos: usize, source: bool) -> (usize, usize) {
        let ref_str = if source {
            &self.source_newlines
        } else {
            &self.templated_newlines
        };
        match ref_str.binary_search(&char_pos) {
            Ok(nl_idx) | Err(nl_idx) => {
                if nl_idx > 0 {
                    (nl_idx + 1, char_pos - ref_str[nl_idx - 1])
                } else {
                    // NB: line_pos is char_pos + 1 because character position is 0-indexed,
                    // but the line position is 1-indexed.
                    (1, char_pos + 1)
                }
            }
        }
    }

    /// Create TemplatedFile from a string.
    pub fn from_string(raw: SmolStr) -> TemplatedFile {
        // TODO: Might need to deal with this unwrap
        TemplatedFile::new(raw.into(), "<string>".to_string(), None, None, None).unwrap()
    }

    /// Get templated string
    pub fn templated(&self) -> &str {
        self.templated_str.as_deref().unwrap()
    }

    pub fn source_only_slices(&self) -> Vec<RawFileSlice> {
        let mut ret_buff = vec![];
        for element in &self.raw_sliced {
            if element.is_source_only_slice() {
                ret_buff.push(element.clone());
            }
        }
        ret_buff
    }

    pub fn find_slice_indices_of_templated_pos(
        &self,
        templated_pos: usize,
        start_idx: Option<usize>,
        inclusive: Option<bool>,
    ) -> Result<(usize, usize), ValueError> {
        let start_idx = start_idx.unwrap_or(0);
        let inclusive = inclusive.unwrap_or(true);

        let mut first_idx: Option<usize> = None;
        let mut last_idx = start_idx;

        // Work through the sliced file, starting at the start_idx if given
        // as an optimisation hint. The sliced_file is a list of TemplatedFileSlice
        // which reference parts of the templated file and where they exist in the
        // source.
        for (idx, elem) in self.sliced_file[start_idx..self.sliced_file.len()]
            .iter()
            .enumerate()
        {
            last_idx = idx + start_idx;
            if elem.templated_slice.end >= templated_pos {
                if first_idx.is_none() {
                    first_idx = Some(idx + start_idx);
                }

                if elem.templated_slice.start > templated_pos
                    || (!inclusive && elem.templated_slice.end >= templated_pos)
                {
                    break;
                }
            }
        }

        // If we got to the end add another index
        if last_idx == self.sliced_file.len() - 1 {
            last_idx += 1;
        }

        match first_idx {
            Some(first_idx) => Ok((first_idx, last_idx)),
            None => Err(ValueError::new("Position Not Found".to_string())),
        }
    }

    /// Convert a template slice to a source slice.
    pub fn templated_slice_to_source_slice(
        &self,
        template_slice: Range<usize>,
    ) -> Result<Range<usize>, ValueError> {
        if self.sliced_file.is_empty() {
            return Ok(template_slice);
        }

        let sliced_file = self.sliced_file.clone();

        let (ts_start_sf_start, ts_start_sf_stop) =
            self.find_slice_indices_of_templated_pos(template_slice.start, None, None)?;

        let ts_start_subsliced_file = &sliced_file[ts_start_sf_start..ts_start_sf_stop];

        // Work out the insertion point
        let mut insertion_point: isize = -1;
        for elem in ts_start_subsliced_file.iter() {
            // Do slice starts and ends
            for &slice_elem in ["start", "stop"].iter() {
                let elem_val = match slice_elem {
                    "start" => elem.templated_slice.start,
                    "stop" => elem.templated_slice.end,
                    _ => panic!("Unexpected slice_elem"),
                };

                if elem_val == template_slice.start {
                    let point = if slice_elem == "start" {
                        elem.source_slice.start
                    } else {
                        elem.source_slice.end
                    };

                    let point: isize = point.try_into().unwrap();
                    if insertion_point < 0 || point < insertion_point {
                        insertion_point = point;
                    }
                    // We don't break here, because we might find ANOTHER
                    // later which is actually earlier.
                }
            }
        }

        // Zero length slice.
        if template_slice.start == template_slice.end {
            // Is it on a join?
            return if insertion_point >= 0 {
                Ok(zero_slice(insertion_point.try_into().unwrap()))
                // It's within a segment.
            } else if !ts_start_subsliced_file.is_empty()
                && ts_start_subsliced_file[0].slice_type == "literal"
            {
                let offset =
                    template_slice.start - ts_start_subsliced_file[0].templated_slice.start;
                Ok(zero_slice(
                    ts_start_subsliced_file[0].source_slice.start + offset,
                ))
            } else {
                Err(ValueError::new(format!(
                    "Attempting a single length slice within a templated section! {:?} within \
                     {:?}.",
                    template_slice, ts_start_subsliced_file
                )))
            };
        }

        let (ts_stop_sf_start, ts_stop_sf_stop) =
            self.find_slice_indices_of_templated_pos(template_slice.end, None, Some(false))?;

        let mut ts_start_sf_start = ts_start_sf_start;
        if insertion_point >= 0 {
            for elem in &sliced_file[ts_start_sf_start..] {
                let insertion_point: usize = insertion_point.try_into().unwrap();
                if elem.source_slice.start != insertion_point {
                    ts_start_sf_start += 1;
                } else {
                    break;
                }
            }
        }

        let subslices = &sliced_file[usize::min(ts_start_sf_start, ts_stop_sf_start)
            ..usize::max(ts_start_sf_stop, ts_stop_sf_stop)];

        let start_slices = if ts_start_sf_start == ts_start_sf_stop {
            return match ts_start_sf_start.cmp(&sliced_file.len()) {
                Ordering::Greater => Err(ValueError::new(
                    "Starting position higher than sliced file position".into(),
                )),
                Ordering::Less => Ok(sliced_file[1].source_slice.clone()),
                Ordering::Equal => Ok(sliced_file.last().unwrap().source_slice.clone()),
            };
        } else {
            &sliced_file[ts_start_sf_start..ts_start_sf_stop]
        };

        let stop_slices = if ts_stop_sf_start == ts_stop_sf_stop {
            vec![sliced_file[ts_stop_sf_start].clone()]
        } else {
            sliced_file[ts_stop_sf_start..ts_stop_sf_stop].to_vec()
        };

        let source_start: isize = if insertion_point >= 0 {
            insertion_point
        } else if start_slices[0].slice_type == "literal" {
            let offset = template_slice.start - start_slices[0].templated_slice.start;
            (start_slices[0].source_slice.start + offset)
                .try_into()
                .unwrap()
        } else {
            start_slices[0].source_slice.start.try_into().unwrap()
        };

        let source_stop = if stop_slices.last().unwrap().slice_type == "literal" {
            let offset = stop_slices.last().unwrap().templated_slice.end - template_slice.end;
            stop_slices.last().unwrap().source_slice.end - offset
        } else {
            stop_slices.last().unwrap().source_slice.end
        };

        let source_slice;
        if source_start > source_stop.try_into().unwrap() {
            let mut source_start = usize::MAX;
            let mut source_stop = 0;
            for elem in subslices {
                source_start = usize::min(source_start, elem.source_slice.start);
                source_stop = usize::max(source_stop, elem.source_slice.end);
            }
            source_slice = source_start..source_stop;
        } else {
            source_slice = source_start.try_into().unwrap()..source_stop;
        }

        Ok(source_slice)
    }

    ///  Work out whether a slice of the source file is a literal or not.
    pub fn is_source_slice_literal(&self, source_slice: &Range<usize>) -> bool {
        // No sliced file? Everything is literal
        if self.raw_sliced.is_empty() {
            return true;
        };

        // Zero length slice. It's a literal, because it's definitely not templated.
        if source_slice.start == source_slice.end {
            return true;
        };

        let mut is_literal = true;
        for raw_slice in &self.raw_sliced {
            // Reset if we find a literal and we're up to the start
            // otherwise set false.
            if raw_slice.source_idx <= source_slice.start {
                is_literal = raw_slice.slice_type == "literal";
            } else if raw_slice.source_idx >= source_slice.end {
                break;
            } else if raw_slice.slice_type != "literal" {
                is_literal = false;
            };
        }
        is_literal
    }

    /// Return a list of the raw slices spanning a set of indices.
    pub(crate) fn raw_slices_spanning_source_slice(
        &self,
        source_slice: &Range<usize>,
    ) -> Vec<RawFileSlice> {
        // Special case: The source_slice is at the end of the file.
        let last_raw_slice = self.raw_sliced.last().unwrap();
        if source_slice.start >= last_raw_slice.source_idx + last_raw_slice.raw.len() {
            return Vec::new();
        }

        // First find the start index
        let mut raw_slice_idx = 0;
        // Move the raw pointer forward to the start of this patch
        while raw_slice_idx + 1 < self.raw_sliced.len()
            && self.raw_sliced[raw_slice_idx + 1].source_idx <= source_slice.start
        {
            raw_slice_idx += 1;
        }

        // Find slice index of the end of this patch.
        let mut slice_span = 1;
        while raw_slice_idx + slice_span < self.raw_sliced.len()
            && self.raw_sliced[raw_slice_idx + slice_span].source_idx < source_slice.end
        {
            slice_span += 1;
        }

        // Return the raw slices
        self.raw_sliced[raw_slice_idx..(raw_slice_idx + slice_span)].to_vec()
    }
}

/// Find the indices of all newlines in a string.
pub fn iter_indices_of_newlines(raw_str: &str) -> impl Iterator<Item = usize> + '_ {
    // TODO: This may be optimize-able by not doing it all up front.
    raw_str.match_indices('\n').map(|(idx, _)| idx)
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum RawFileSliceType {
    Comment,
    BlockEnd,
    BlockStart,
    BlockMid,
}

/// A slice referring to a raw file.
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct RawFileSlice {
    /// Source string
    pub raw: String,
    pub slice_type: String,
    /// Offset from beginning of source string
    pub source_idx: usize,
    slice_subtype: Option<RawFileSliceType>,
    /// Block index, incremented on start or end block tags, e.g. "if", "for"
    block_idx: usize,
}

impl RawFileSlice {
    pub fn new(
        raw: String,
        slice_type: String,
        source_idx: usize,
        slice_subtype: Option<RawFileSliceType>,
        block_idx: Option<usize>,
    ) -> Self {
        Self {
            raw,
            slice_type,
            source_idx,
            slice_subtype,
            block_idx: block_idx.unwrap_or(0),
        }
    }
}

impl RawFileSlice {
    /// Return the closing index of this slice.
    fn end_source_idx(&self) -> usize {
        self.source_idx + self.raw.len()
    }

    /// Return the a slice object for this slice.
    pub fn source_slice(&self) -> Range<usize> {
        self.source_idx..self.end_source_idx()
    }

    /// Based on its slice_type, does it only appear in the *source*?
    /// There are some slice types which are automatically source only.
    /// There are *also* some which are source only because they render
    /// to an empty string.
    fn is_source_only_slice(&self) -> bool {
        // TODO: should any new logic go here?. Slice Type could probably go from String
        // To Enum
        matches!(
            self.slice_type.as_str(),
            "comment" | "block_end" | "block_start" | "block_mid"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indices_of_newlines() {
        vec![
            ("", vec![]),
            ("foo", vec![]),
            ("foo\nbar", vec![3]),
            ("\nfoo\n\nbar\nfoo\n\nbar\n", vec![0, 4, 5, 9, 13, 14, 18]),
        ]
        .into_iter()
        .for_each(|(in_str, expected)| {
            assert_eq!(
                expected,
                iter_indices_of_newlines(in_str).collect::<Vec<usize>>()
            )
        });
    }

    // const SIMPLE_SOURCE_STR: &str = "01234\n6789{{foo}}fo\nbarss";
    // const SIMPLE_TEMPLATED_STR: &str = "01234\n6789x\nfo\nbarfss";

    fn simple_sliced_file() -> Vec<TemplatedFileSlice> {
        vec![
            TemplatedFileSlice::new("literal", 0..10, 0..10),
            TemplatedFileSlice::new("templated", 10..17, 10..12),
            TemplatedFileSlice::new("literal", 17..25, 12..20),
        ]
    }

    fn simple_raw_sliced_file() -> [RawFileSlice; 3] {
        [
            RawFileSlice::new("x".repeat(10), "literal".to_string(), 0, None, None),
            RawFileSlice::new("x".repeat(7), "templated".to_string(), 10, None, None),
            RawFileSlice::new("x".repeat(8), "literal".to_string(), 17, None, None),
        ]
    }

    fn complex_sliced_file() -> Vec<TemplatedFileSlice> {
        vec![
            TemplatedFileSlice::new("literal", 0..13, 0..13),
            TemplatedFileSlice::new("comment", 13..29, 13..13),
            TemplatedFileSlice::new("literal", 29..44, 13..28),
            TemplatedFileSlice::new("block_start", 44..68, 28..28),
            TemplatedFileSlice::new("literal", 68..81, 28..41),
            TemplatedFileSlice::new("templated", 81..86, 41..42),
            TemplatedFileSlice::new("literal", 86..110, 42..66),
            TemplatedFileSlice::new("templated", 68..86, 66..76),
            TemplatedFileSlice::new("literal", 68..81, 76..89),
            TemplatedFileSlice::new("templated", 81..86, 89..90),
            TemplatedFileSlice::new("literal", 86..110, 90..114),
            TemplatedFileSlice::new("templated", 68..86, 114..125),
            TemplatedFileSlice::new("literal", 68..81, 125..138),
            TemplatedFileSlice::new("templated", 81..86, 138..139),
            TemplatedFileSlice::new("literal", 86..110, 139..163),
            TemplatedFileSlice::new("templated", 110..123, 163..166),
            TemplatedFileSlice::new("literal", 123..132, 166..175),
            TemplatedFileSlice::new("block_end", 132..144, 175..175),
            TemplatedFileSlice::new("literal", 144..155, 175..186),
            TemplatedFileSlice::new("block_start", 155..179, 186..186),
            TemplatedFileSlice::new("literal", 179..189, 186..196),
            TemplatedFileSlice::new("templated", 189..194, 196..197),
            TemplatedFileSlice::new("literal", 194..203, 197..206),
            TemplatedFileSlice::new("literal", 179..189, 206..216),
            TemplatedFileSlice::new("templated", 189..194, 216..217),
            TemplatedFileSlice::new("literal", 194..203, 217..226),
            TemplatedFileSlice::new("literal", 179..189, 226..236),
            TemplatedFileSlice::new("templated", 189..194, 236..237),
            TemplatedFileSlice::new("literal", 194..203, 237..246),
            TemplatedFileSlice::new("block_end", 203..215, 246..246),
            TemplatedFileSlice::new("literal", 215..230, 246..261),
        ]
    }

    fn complex_raw_sliced_file() -> Vec<RawFileSlice> {
        vec![
            RawFileSlice::new(
                "x".repeat(13).to_string(),
                "literal".to_string(),
                0,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(16).to_string(),
                "comment".to_string(),
                13,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(15).to_string(),
                "literal".to_string(),
                29,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(24).to_string(),
                "block_start".to_string(),
                44,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(13).to_string(),
                "literal".to_string(),
                68,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(5).to_string(),
                "templated".to_string(),
                81,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(24).to_string(),
                "literal".to_string(),
                86,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(13).to_string(),
                "templated".to_string(),
                110,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(9).to_string(),
                "literal".to_string(),
                123,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(12).to_string(),
                "block_end".to_string(),
                132,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(11).to_string(),
                "literal".to_string(),
                144,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(24).to_string(),
                "block_start".to_string(),
                155,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(10).to_string(),
                "literal".to_string(),
                179,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(5).to_string(),
                "templated".to_string(),
                189,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(9).to_string(),
                "literal".to_string(),
                194,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(12).to_string(),
                "block_end".to_string(),
                203,
                None,
                None,
            ),
            RawFileSlice::new(
                "x".repeat(15).to_string(),
                "literal".to_string(),
                215,
                None,
                None,
            ),
        ]
    }

    struct FileKwargs {
        f_name: String,
        source_str: String,
        templated_str: Option<String>,
        sliced_file: Vec<TemplatedFileSlice>,
        raw_sliced_file: Vec<RawFileSlice>,
    }

    fn simple_file_kwargs() -> FileKwargs {
        FileKwargs {
            f_name: "test.sql".to_string(),
            source_str: "01234\n6789{{foo}}fo\nbarss".to_string(),
            templated_str: Some("01234\n6789x\nfo\nbarss".to_string()),
            sliced_file: simple_sliced_file().to_vec(),
            raw_sliced_file: simple_raw_sliced_file().to_vec(),
        }
    }

    fn complex_file_kwargs() -> FileKwargs {
        FileKwargs {
            f_name: "test.sql".to_string(),
            source_str: complex_raw_sliced_file()
                .iter()
                .fold(String::new(), |acc, x| acc + &x.raw),
            templated_str: None,
            sliced_file: complex_sliced_file().to_vec(),
            raw_sliced_file: complex_raw_sliced_file().to_vec(),
        }
    }

    #[test]
    /// Test TemplatedFile.get_line_pos_of_char_pos.
    fn test_templated_file_get_line_pos_of_char_pos() {
        let tests = [
            (simple_file_kwargs(), 0, 1, 1),
            (simple_file_kwargs(), 20, 3, 1),
            (simple_file_kwargs(), 24, 3, 5),
        ];

        for test in tests {
            let kwargs = test.0;

            let tf = TemplatedFile::new(
                kwargs.source_str,
                kwargs.f_name,
                kwargs.templated_str,
                Some(kwargs.sliced_file),
                Some(kwargs.raw_sliced_file),
            )
            .unwrap();

            let (res_line_no, res_line_pos) = tf.get_line_pos_of_char_pos(test.1, true);

            assert_eq!(res_line_no, test.2);
            assert_eq!(res_line_pos, test.3);
        }
    }

    #[test]
    fn test_templated_file_find_slice_indices_of_templated_pos() {
        let tests = vec![
            // "templated_position,inclusive,file_slices,sliced_idx_start,sliced_idx_stop",
            // TODO Fix these
            // (100, true, complex_file_kwargs(), 10, 11),
            // (13, true, complex_file_kwargs(), 0, 3),
            // (28, true, complex_file_kwargs(), 2, 5),
            // # Check end slicing.
            (12, true, simple_file_kwargs(), 1, 3),
            (20, true, simple_file_kwargs(), 2, 3),
            // Check inclusivity
            // (13, false, complex_file_kwargs(), 0, 1),
        ];

        for test in tests {
            let args = test.2;

            let file = TemplatedFile::new(
                args.source_str,
                args.f_name,
                args.templated_str,
                Some(args.sliced_file),
                Some(args.raw_sliced_file),
            )
            .unwrap();

            let (res_start, res_stop) = file
                .find_slice_indices_of_templated_pos(test.0, None, Some(test.1))
                .unwrap();

            assert_eq!(res_start, test.3);
            assert_eq!(res_stop, test.4);
        }
    }

    #[test]
    /// Test TemplatedFile.templated_slice_to_source_slice
    fn test_templated_file_templated_slice_to_source_slice() {
        let test_cases = vec![
            // Simple example
            (
                5..10,
                5..10,
                true,
                FileKwargs {
                    sliced_file: vec![TemplatedFileSlice::new("literal", 0..20, 0..20)],
                    raw_sliced_file: vec![RawFileSlice::new(
                        "x".repeat(20),
                        "literal".to_string(),
                        0,
                        None,
                        None,
                    )],
                    source_str: "x".repeat(20),
                    f_name: "foo.sql".to_string(),
                    templated_str: None,
                },
            ),
            // Trimming the end of a literal (with things that follow).
            (10..13, 10..13, true, complex_file_kwargs()),
            // // Unrealistic, but should still work
            (
                5..10,
                55..60,
                true,
                FileKwargs {
                    sliced_file: vec![TemplatedFileSlice::new("literal", 50..70, 0..20)],
                    raw_sliced_file: vec![
                        RawFileSlice::new("x".repeat(50), "literal".to_string(), 0, None, None),
                        RawFileSlice::new("x".repeat(20), "literal".to_string(), 50, None, None),
                    ],
                    source_str: "x".repeat(70),
                    f_name: "foo.sql".to_string(),
                    templated_str: None,
                },
            ),
            // // Spanning a template
            (5..15, 5..20, false, simple_file_kwargs()),
            // // Handling templated
            (
                5..15,
                0..25,
                false,
                FileKwargs {
                    sliced_file: simple_file_kwargs()
                        .sliced_file
                        .iter()
                        .map(|slc| {
                            TemplatedFileSlice::new(
                                "templated",
                                slc.source_slice.clone(),
                                slc.templated_slice.clone(),
                            )
                        })
                        .collect(),
                    raw_sliced_file: simple_file_kwargs()
                        .raw_sliced_file
                        .iter()
                        .map(|slc| {
                            RawFileSlice::new(
                                slc.raw.to_string(),
                                "templated".to_string(),
                                slc.source_idx,
                                None,
                                None,
                            )
                        })
                        .collect(),
                    ..simple_file_kwargs()
                },
            ),
            // // Handling single length slices
            (10..10, 10..10, true, simple_file_kwargs()),
            (12..12, 17..17, true, simple_file_kwargs()),
            // // Dealing with single length elements
            (
                20..20,
                25..25,
                true,
                FileKwargs {
                    sliced_file: simple_file_kwargs()
                        .sliced_file
                        .into_iter()
                        .chain(vec![TemplatedFileSlice::new("comment", 25..35, 20..20)])
                        .collect(),
                    raw_sliced_file: simple_file_kwargs()
                        .raw_sliced_file
                        .into_iter()
                        .chain(vec![RawFileSlice::new(
                            "x".repeat(10),
                            "comment".to_string(),
                            25,
                            None,
                            None,
                        )])
                        .collect(),
                    source_str: simple_file_kwargs().source_str.to_string() + &"x".repeat(10),
                    ..simple_file_kwargs()
                },
            ),
            // // Just more test coverage
            (43..43, 87..87, true, complex_file_kwargs()),
            (13..13, 13..13, true, complex_file_kwargs()),
            (186..186, 155..155, true, complex_file_kwargs()),
            // Backward slicing.
            (
                100..130,
                // NB This actually would reference the wrong way around if we
                // just take the points. Here we should handle it gracefully.
                68..110,
                false,
                complex_file_kwargs(),
            ),
        ];

        for (in_slice, out_slice, is_literal, tf_kwargs) in test_cases {
            let file = TemplatedFile::new(
                tf_kwargs.source_str,
                tf_kwargs.f_name,
                tf_kwargs.templated_str,
                Some(tf_kwargs.sliced_file),
                Some(tf_kwargs.raw_sliced_file),
            )
            .unwrap();

            let source_slice = file.templated_slice_to_source_slice(in_slice).unwrap();
            let literal_test = file.is_source_slice_literal(&source_slice);

            assert_eq!((is_literal, source_slice), (literal_test, out_slice));
        }
    }

    #[test]
    /// Test TemplatedFile.source_only_slices
    fn test_templated_file_source_only_slices() {
        let test_cases = vec![
            // Comment example
            (
                TemplatedFile::new(
                    format!("{}{}{}", "a".repeat(10), "{# b #}", "a".repeat(10)),
                    "test".to_string(),
                    None,
                    Some(vec![
                        TemplatedFileSlice::new("literal", 0..10, 0..10),
                        TemplatedFileSlice::new("templated", 10..17, 10..10),
                        TemplatedFileSlice::new("literal", 17..27, 10..20),
                    ]),
                    Some(vec![
                        RawFileSlice::new(
                            "a".repeat(10).to_string(),
                            "literal".to_string(),
                            0,
                            None,
                            None,
                        ),
                        RawFileSlice::new(
                            "{# b #}".to_string(),
                            "comment".to_string(),
                            10,
                            None,
                            None,
                        ),
                        RawFileSlice::new(
                            "a".repeat(10).to_string(),
                            "literal".to_string(),
                            17,
                            None,
                            None,
                        ),
                    ]),
                )
                .unwrap(),
                vec![RawFileSlice::new(
                    "{# b #}".to_string(),
                    "comment".to_string(),
                    10,
                    None,
                    None,
                )],
            ),
            // Template tags aren't source only.
            (
                TemplatedFile::new(
                    "aaa{{ b }}aaa".to_string(),
                    "test".to_string(),
                    None,
                    Some(vec![
                        TemplatedFileSlice::new("literal", 0..3, 0..3),
                        TemplatedFileSlice::new("templated", 3..10, 3..6),
                        TemplatedFileSlice::new("literal", 10..13, 6..9),
                    ]),
                    Some(vec![
                        RawFileSlice::new("aaa".to_string(), "literal".to_string(), 0, None, None),
                        RawFileSlice::new(
                            "{{ b }}".to_string(),
                            "templated".to_string(),
                            3,
                            None,
                            None,
                        ),
                        RawFileSlice::new("aaa".to_string(), "literal".to_string(), 10, None, None),
                    ]),
                )
                .unwrap(),
                vec![],
            ),
        ];

        for (file, expected) in test_cases {
            assert_eq!(file.source_only_slices(), expected, "Failed for {:?}", file);
        }
    }
}

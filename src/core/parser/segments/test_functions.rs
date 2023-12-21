use std::ops::Range;

use crate::core::config::FluffConfig;
use crate::core::dialects::base::Dialect;
use crate::core::dialects::init::dialect_selector;
use crate::core::parser::lexer::{Lexer, StringOrTemplate};
use crate::core::parser::markers::PositionMarker;
use crate::core::parser::segments::base::{
    CodeSegment, CodeSegmentNewArgs, CommentSegment, CommentSegmentNewArgs, NewlineSegment,
    NewlineSegmentNewArgs, Segment, SymbolSegment, SymbolSegmentNewArgs, WhitespaceSegment,
    WhitespaceSegmentNewArgs,
};
use crate::core::parser::segments::meta::{Dedent, Indent};
use crate::core::templaters::base::TemplatedFile;
use crate::helpers::Boxed;

use super::keyword::KeywordSegment;

pub fn fresh_ansi_dialect() -> Dialect {
    dialect_selector("ansi").unwrap()
}

pub fn lex(string: &str) -> Vec<Box<dyn Segment>> {
    let config = FluffConfig::new(None, None, None, None);
    let lexer = Lexer::new(config, None);

    let (segments, errors) = lexer.lex(StringOrTemplate::String(string.into())).unwrap();
    assert_eq!(errors, &[]);
    segments
}

/// Roughly generate test segments.
///
/// This is a factory function so that it works as a fixture,
/// but when actually used, this will return the inner function
/// which is what you actually need.
pub fn generate_test_segments_func(elems: Vec<&str>) -> Vec<Box<dyn Segment>> {
    // Placeholder: assuming TemplatedFile, PositionMarker, and other structures
    // are defined elsewhere in the codebase.
    let raw_file = elems.concat();

    let templated_file = TemplatedFile::from_string(raw_file);
    let mut idx = 0;
    let mut buff = Vec::new();

    for elem in elems {
        if elem == "<indent>" {
            buff.push(
                Indent::new(PositionMarker::from_point(
                    idx,
                    idx,
                    templated_file.clone(),
                    None,
                    None,
                ))
                .boxed() as Box<dyn Segment>,
            );
            continue;
        } else if elem == "<dedent>" {
            buff.push(Dedent::new(PositionMarker::from_point(
                idx,
                idx,
                templated_file.clone(),
                None,
                None,
            )));
            continue;
        }

        let position_marker = PositionMarker::new(
            idx..idx + elem.len(),
            idx..idx + elem.len(),
            templated_file.clone(),
            None,
            None,
        );

        let seg = if elem.chars().all(|c| c == ' ' || c == '\t') {
            WhitespaceSegment::new(elem, &position_marker, WhitespaceSegmentNewArgs {})
        } else if elem.chars().all(|c| c == '\n') {
            NewlineSegment::new(elem, &position_marker, NewlineSegmentNewArgs {})
        } else if elem == "(" || elem == ")" {
            SymbolSegment::new(elem, &position_marker, SymbolSegmentNewArgs {})
        } else if elem.starts_with("--") {
            CommentSegment::new(
                elem,
                &position_marker,
                CommentSegmentNewArgs {
                    r#type: "inline",
                    trim_start: None,
                },
            )
        } else if elem.starts_with('\"') {
            CodeSegment::new(
                elem,
                &position_marker,
                CodeSegmentNewArgs {
                    code_type: "double_quote",
                    instance_types: vec![],
                    trim_start: None,
                    trim_chars: None,
                    source_fixes: None,
                },
            )
        } else if elem.starts_with('\'') {
            CodeSegment::new(
                elem,
                &position_marker,
                CodeSegmentNewArgs {
                    code_type: "single_quote",
                    instance_types: vec![],
                    trim_start: None,
                    trim_chars: None,
                    source_fixes: None,
                },
            )
        } else {
            CodeSegment::new(
                elem,
                &position_marker,
                CodeSegmentNewArgs {
                    code_type: "",
                    instance_types: vec![],
                    trim_start: None,
                    trim_chars: None,
                    source_fixes: None,
                },
            )
        };

        buff.push(seg);
        idx += elem.len();
    }

    buff
}

/// Construct a list of raw segments as a fixture.
pub fn raw_segments() -> Vec<Box<dyn Segment>> {
    return generate_test_segments_func(["foobar", ".barfoo"].to_vec());
}

pub fn raw_seg() -> Box<dyn Segment> {
    raw_segments()[0].clone()
}

pub fn test_segments() -> Vec<Box<dyn Segment>> {
    generate_test_segments_func(vec!["bar", " \t ", "foo", "baar", " \t "])
}

pub fn make_result_tuple(
    result_slice: Option<Range<usize>>,
    matcher_keywords: &[&str],
    test_segments: &[Box<dyn Segment>],
) -> Vec<Box<dyn Segment>> {
    // Make a comparison tuple for test matching.
    // No result slice means no match.
    match result_slice {
        None => vec![],
        Some(slice) => test_segments[slice]
            .iter()
            .filter_map(|elem| {
                elem.get_raw().map(|raw| {
                    if matcher_keywords.contains(&raw.as_str()) {
                        KeywordSegment::new(raw, elem.get_position_marker().unwrap()).boxed()
                            as Box<dyn Segment>
                    } else {
                        elem.clone()
                    }
                })
            })
            .collect(),
    }
}

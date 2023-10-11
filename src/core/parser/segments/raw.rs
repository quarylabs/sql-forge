use crate::core::parser::markers::PositionMarker;
use crate::core::parser::segments::base::Segment;
use crate::core::parser::segments::fix::SourceFix;

#[derive(Debug, Clone)]
pub struct RawSegment {
    raw: Option<String>,
    position_marker: Option<PositionMarker>,
}

impl RawSegment {
    pub fn new(
        raw: Option<String>,
        position_marker: Option<PositionMarker>,
        // For legacy and syntactic sugar we allow the simple
        // `type` argument here, but for more precise inheritance
        // we suggest using the `instance_types` option.
        _type: Option<String>,
        _instance_types: Option<Vec<String>>,
        _trim_start: Option<Vec<String>>,
        _trim_cars: Option<Vec<String>>,
        _source_fixes: Option<Vec<SourceFix>>,
        _uuid: Option<String>,
    ) -> Self {
        Self {
            position_marker,
            raw,
        }
    }
}

impl Segment for RawSegment {
    fn get_type(&self) -> &'static str {
        "raw"
    }

    fn is_code(&self) -> bool {
        true
    }

    fn is_comment(&self) -> bool {
        false
    }

    fn is_whitespace(&self) -> bool {
        false
    }

    fn get_raw(&self) -> Option<String> {
        self.raw.clone()
    }

    fn get_pos_maker(&self) -> Option<PositionMarker> {
        self.position_marker.clone()
    }

    fn get_raw_segments(&self) -> Option<Vec<Box<dyn Segment>>> {
        return Some(vec![Box::new(self.clone())]);
    }
}

#[cfg(test)]
mod test {
    // Test niche case of calling get_raw_segments on a raw segment.
    // TODO Implement
    // #[test]
    // fn test__parser__raw_get_raw_segments() {
    //     let segs = raw_segments();
    //
    //     for seg in segs {
    //         assert_eq!(seg.get_raw_segments(), Some(vec![seg.clone()]));
    //     }
    // }
}

use std::any::Any;
use std::borrow::Cow;
use std::cell::{Cell, OnceCell};
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use dyn_ord::DynEq;
use itertools::{enumerate, Itertools};
use rustc_hash::FxHashMap;
use smol_str::SmolStr;

use crate::core::dialects::init::DialectKind;
use crate::core::parser::markers::PositionMarker;
use crate::core::parser::segments::fix::{AnchorEditInfo, FixPatch, SourceFix};
use crate::core::parser::segments::object_reference::{
    ObjectReferenceKind, ObjectReferenceSegment,
};
use crate::core::rules::base::EditType;
use crate::core::templaters::base::TemplatedFile;
use crate::dialects::{SyntaxKind, SyntaxSet};

pub struct SegmentBuilder {
    node_or_token: NodeOrToken,
}

impl SegmentBuilder {
    pub fn whitespace(id: u32, raw: &str) -> ErasedSegment {
        SegmentBuilder::token(id, raw, SyntaxKind::Whitespace).finish()
    }

    pub fn newline(id: u32, raw: &str) -> ErasedSegment {
        SegmentBuilder::token(id, raw, SyntaxKind::Newline).finish()
    }

    pub fn keyword(id: u32, raw: &str) -> ErasedSegment {
        SegmentBuilder::token(id, raw, SyntaxKind::Keyword).finish()
    }

    pub fn symbol(id: u32, raw: &str) -> ErasedSegment {
        SegmentBuilder::token(id, raw, SyntaxKind::Symbol).finish()
    }

    pub fn node(
        id: u32,
        syntax_kind: SyntaxKind,
        dialect: DialectKind,
        segments: Vec<ErasedSegment>,
    ) -> Self {
        SegmentBuilder {
            node_or_token: NodeOrToken {
                id,
                syntax_kind,
                class_types: class_types(syntax_kind),
                position_marker: None,
                kind: NodeOrTokenKind::Node(NodeData {
                    dialect,
                    segments,
                    raw: Default::default(),
                    source_fixes: vec![],
                    descendant_type_set: Default::default(),
                    raw_segments_with_ancestors: Default::default(),
                }),
            },
        }
    }

    pub fn token(id: u32, raw: &str, syntax_kind: SyntaxKind) -> Self {
        SegmentBuilder {
            node_or_token: NodeOrToken {
                id,
                syntax_kind,
                class_types: class_types(syntax_kind),
                position_marker: None,
                kind: NodeOrTokenKind::Token(TokenData { raw: raw.into() }),
            },
        }
    }

    pub fn position_from_segments(mut self) -> Self {
        let segments = match &self.node_or_token.kind {
            NodeOrTokenKind::Node(node) => &node.segments[..],
            NodeOrTokenKind::Token(_) => &[],
        };

        self.node_or_token.position_marker = pos_marker(segments).into();
        self
    }

    pub fn with_position(mut self, position: PositionMarker) -> Self {
        self.node_or_token.position_marker = Some(position);
        self
    }

    pub fn finish(self) -> ErasedSegment {
        ErasedSegment { value: Rc::new(self.node_or_token), hash: Rc::new(AtomicU64::new(0)) }
    }
}

#[derive(Debug, Default)]
pub struct Tables {
    counter: Cell<u32>,
}

impl Tables {
    pub(crate) fn next_id(&self) -> u32 {
        let id = self.counter.get();
        self.counter.set(id + 1);
        id
    }
}

#[derive(Debug, Clone)]
pub struct ErasedSegment {
    pub(crate) value: Rc<NodeOrToken>,
    pub(crate) hash: Rc<AtomicU64>,
}

impl Hash for ErasedSegment {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash_value().hash(state);
    }
}

impl Eq for ErasedSegment {}

impl ErasedSegment {
    pub fn raw(&self) -> Cow<str> {
        match &self.value.kind {
            NodeOrTokenKind::Node(node) => node
                .raw
                .get_or_init(|| self.segments().iter().map(|segment| segment.raw()).join(""))
                .into(),
            NodeOrTokenKind::Token(token) => token.raw.as_str().into(),
        }
    }

    pub fn segments(&self) -> &[ErasedSegment] {
        match &self.value.kind {
            NodeOrTokenKind::Node(node) => &node.segments,
            NodeOrTokenKind::Token(_) => &[],
        }
    }

    pub fn get_type(&self) -> SyntaxKind {
        self.value.syntax_kind
    }

    pub fn is_type(&self, kind: SyntaxKind) -> bool {
        self.get_type() == kind
    }

    pub fn is_meta(&self) -> bool {
        matches!(
            self.value.syntax_kind,
            SyntaxKind::Indent | SyntaxKind::Implicit | SyntaxKind::Dedent | SyntaxKind::EndOfFile
        )
    }

    pub fn is_code(&self) -> bool {
        match &self.value.kind {
            NodeOrTokenKind::Node(node) => node.segments.iter().any(|s| s.is_code()),
            NodeOrTokenKind::Token(_) => {
                !self.is_comment() && !self.is_whitespace() && !self.is_meta()
            }
        }
    }

    pub fn get_raw_segments(&self) -> Vec<ErasedSegment> {
        self.recursive_crawl_all(false).into_iter().filter(|it| it.segments().is_empty()).collect()
    }

    #[cfg(feature = "stringify")]
    pub fn stringify(&self) -> String {
        serde_yaml::to_string(&self.to_serialised(true, true)).unwrap()
    }

    pub fn child(&self, seg_types: &SyntaxSet) -> Option<ErasedSegment> {
        self.segments().iter().find(|seg| seg_types.contains(seg.get_type())).cloned()
    }

    pub fn recursive_crawl(
        &self,
        types: &SyntaxSet,
        recurse_into: bool,
        no_recursive_types: &SyntaxSet,
        allow_self: bool,
    ) -> Vec<ErasedSegment> {
        let mut acc = Vec::new();

        let matches = allow_self && self.class_types().intersects(types);
        if matches {
            acc.push(self.clone());
        }

        if !self.descendant_type_set().intersects(types) {
            return acc;
        }

        if recurse_into || !matches {
            for seg in self.segments() {
                if no_recursive_types.is_empty() || !no_recursive_types.contains(seg.get_type()) {
                    let segments =
                        seg.recursive_crawl(types, recurse_into, no_recursive_types, true);
                    acc.extend(segments);
                }
            }
        }

        acc
    }
}

impl ErasedSegment {
    #[allow(clippy::new_ret_no_self, clippy::wrong_self_convention)]
    #[track_caller]
    pub fn new(&self, segments: Vec<ErasedSegment>) -> ErasedSegment {
        match &self.value.kind {
            NodeOrTokenKind::Node(node) => {
                SegmentBuilder::node(self.value.id, self.value.syntax_kind, node.dialect, segments)
                    .with_position(self.get_position_marker().unwrap().clone())
                    .finish()
            }
            NodeOrTokenKind::Token(_) => self.deep_clone(),
        }
    }

    fn change_segments(&self, segments: Vec<ErasedSegment>) -> ErasedSegment {
        let NodeOrTokenKind::Node(node) = &self.value.kind else { unimplemented!() };

        ErasedSegment {
            value: Rc::new(NodeOrToken {
                id: self.value.id,
                syntax_kind: self.value.syntax_kind,
                class_types: self.value.class_types.clone(),
                position_marker: None,
                kind: NodeOrTokenKind::Node(NodeData {
                    dialect: node.dialect,
                    segments,
                    raw: node.raw.clone(),
                    source_fixes: node.source_fixes.clone(),
                    descendant_type_set: node.descendant_type_set.clone(),
                    raw_segments_with_ancestors: node.raw_segments_with_ancestors.clone(),
                }),
            }),
            hash: self.hash.clone(),
        }
    }

    pub(crate) fn indent_val(&self) -> i8 {
        self.value.syntax_kind.indent_val()
    }

    pub(crate) fn can_start_end_non_code(&self) -> bool {
        matches!(self.value.syntax_kind, SyntaxKind::File | SyntaxKind::Unparsable)
    }

    pub(crate) fn dialect(&self) -> DialectKind {
        match &self.value.kind {
            NodeOrTokenKind::Node(node) => node.dialect,
            NodeOrTokenKind::Token(_) => todo!(),
        }
    }

    pub(crate) fn get_start_loc(&self) -> (usize, usize) {
        match self.get_position_marker() {
            Some(pos_marker) => pos_marker.working_loc(),
            None => unreachable!("{self:?} has no PositionMarker"),
        }
    }

    pub(crate) fn get_end_loc(&self) -> (usize, usize) {
        match self.get_position_marker() {
            Some(pos_marker) => pos_marker.working_loc_after(&self.raw()),
            None => {
                unreachable!("{self:?} has no PositionMarker")
            }
        }
    }

    pub(crate) fn select_children(
        &self,
        start_seg: Option<&ErasedSegment>,
        stop_seg: Option<&ErasedSegment>,
        select_if: Option<fn(&ErasedSegment) -> bool>,
        loop_while: Option<fn(&ErasedSegment) -> bool>,
    ) -> Vec<ErasedSegment> {
        let segments = self.segments();

        let start_index = start_seg
            .and_then(|seg| segments.iter().position(|x| x.dyn_eq(seg)))
            .map_or(0, |index| index + 1);

        let stop_index = stop_seg
            .and_then(|seg| segments.iter().position(|x| x.dyn_eq(seg)))
            .unwrap_or(segments.len());

        let mut buff = Vec::new();

        for seg in segments.iter().skip(start_index).take(stop_index - start_index) {
            if let Some(loop_while) = &loop_while {
                if !loop_while(seg) {
                    break;
                }
            }

            if select_if.as_ref().map_or(true, |f| f(seg)) {
                buff.push(seg.clone());
            }
        }

        buff
    }

    pub(crate) fn is_templated(&self) -> bool {
        if let Some(pos_marker) = self.get_position_marker() {
            pos_marker.source_slice.start != pos_marker.source_slice.end && !pos_marker.is_literal()
        } else {
            panic!("PosMarker must be set");
        }
    }

    pub(crate) fn iter_segments(
        &self,
        expanding: Option<&SyntaxSet>,
        pass_through: bool,
    ) -> Vec<ErasedSegment> {
        let mut result = Vec::new();
        for s in self.gather_segments() {
            if let Some(expanding) = expanding {
                if expanding.contains(s.get_type()) {
                    result.extend(
                        s.iter_segments(if pass_through { Some(expanding) } else { None }, false),
                    );
                } else {
                    result.push(s);
                }
            } else {
                result.push(s);
            }
        }
        result
    }

    pub(crate) fn code_indices(&self) -> Vec<usize> {
        self.segments()
            .iter()
            .enumerate()
            .filter(|(_, seg)| seg.is_code())
            .map(|(idx, _)| idx)
            .collect()
    }

    pub(crate) fn children(&self, seg_types: &SyntaxSet) -> Vec<ErasedSegment> {
        let mut buff = Vec::new();
        for seg in self.segments() {
            if seg_types.contains(seg.get_type()) {
                buff.push(seg.clone());
            }
        }
        buff
    }

    pub(crate) fn iter_patches(&self, templated_file: &TemplatedFile) -> Vec<FixPatch> {
        let mut acc = Vec::new();

        let templated_raw = &templated_file.templated_str.as_ref().unwrap()
            [self.get_position_marker().unwrap().templated_slice.clone()];
        if self.raw() == templated_raw {
            acc.extend(self.iter_source_fix_patches(templated_file));
            return acc;
        }

        if self.get_position_marker().is_none() {
            return Vec::new();
        }

        let pos_marker = self.get_position_marker().unwrap();
        if pos_marker.is_literal() {
            acc.extend(self.iter_source_fix_patches(templated_file));
            acc.push(FixPatch::new(
                pos_marker.templated_slice.clone(),
                self.raw().into(),
                // SyntaxKind::Literal.into(),
                pos_marker.source_slice.clone(),
                templated_file.templated_str.as_ref().unwrap()[pos_marker.templated_slice.clone()]
                    .to_string(),
                templated_file.source_str[pos_marker.source_slice.clone()].to_string(),
            ));
        } else if self.segments().is_empty() {
            return acc;
        } else {
            let mut segments = self.segments();

            while !segments.is_empty()
                && matches!(
                    segments.last().unwrap().get_type(),
                    SyntaxKind::EndOfFile
                        | SyntaxKind::Indent
                        | SyntaxKind::Dedent
                        | SyntaxKind::Implicit
                )
            {
                segments = &segments[..segments.len() - 1];
            }

            let pos = self.get_position_marker().unwrap();
            let mut source_idx = pos.source_slice.start;
            let mut templated_idx = pos.templated_slice.start;
            let mut insert_buff = String::new();

            for segment in segments {
                let pos_marker = segment.get_position_marker().unwrap();
                if !segment.raw().is_empty() && pos_marker.is_point() {
                    insert_buff.push_str(segment.raw().as_ref());
                    continue;
                }

                let start_diff = pos_marker.templated_slice.start - templated_idx;

                if start_diff > 0 || !insert_buff.is_empty() {
                    let fixed_raw = std::mem::take(&mut insert_buff);
                    let raw_segments = segment.get_raw_segments();
                    let first_segment_pos = raw_segments[0].get_position_marker().unwrap();

                    acc.push(FixPatch::new(
                        templated_idx..first_segment_pos.templated_slice.start,
                        fixed_raw.into(),
                        source_idx..first_segment_pos.source_slice.start,
                        String::new(),
                        String::new(),
                    ));
                }

                acc.extend(segment.iter_patches(templated_file));

                source_idx = pos_marker.source_slice.end;
                templated_idx = pos_marker.templated_slice.end;
            }

            let end_diff = pos.templated_slice.end - templated_idx;
            if end_diff != 0 || !insert_buff.is_empty() {
                let source_slice = source_idx..pos.source_slice.end;
                let templated_slice = templated_idx..pos.templated_slice.end;

                let templated_str = templated_file.templated_str.as_ref().unwrap()
                    [templated_slice.clone()]
                .to_owned();
                let source_str = templated_file.source_str[source_slice.clone()].to_owned();

                acc.push(FixPatch::new(
                    templated_slice,
                    insert_buff.into(),
                    source_slice,
                    templated_str,
                    source_str,
                ));
            }
        }

        acc
    }

    pub(crate) fn descendant_type_set(&self) -> &SyntaxSet {
        match &self.value.kind {
            NodeOrTokenKind::Node(node) => node.descendant_type_set.get_or_init(|| {
                self.segments()
                    .iter()
                    .flat_map(|segment| {
                        segment.descendant_type_set().clone().union(segment.class_types())
                    })
                    .collect()
            }),
            NodeOrTokenKind::Token(_) => const { &SyntaxSet::EMPTY },
        }
    }

    pub(crate) fn get_raw_upper(&self) -> Option<String> {
        self.raw().to_uppercase().into()
    }

    pub(crate) fn is_comment(&self) -> bool {
        matches!(
            self.value.syntax_kind,
            SyntaxKind::Comment | SyntaxKind::InlineComment | SyntaxKind::BlockComment
        )
    }

    pub(crate) fn is_whitespace(&self) -> bool {
        matches!(self.value.syntax_kind, SyntaxKind::Whitespace | SyntaxKind::Newline)
    }

    pub(crate) fn is_indent(&self) -> bool {
        matches!(
            self.value.syntax_kind,
            SyntaxKind::Indent | SyntaxKind::Implicit | SyntaxKind::Dedent
        )
    }

    pub(crate) fn get_position_marker(&self) -> Option<&PositionMarker> {
        self.value.position_marker.as_ref()
    }

    pub(crate) fn gather_segments(&self) -> Vec<ErasedSegment> {
        self.segments().to_vec()
    }

    pub(crate) fn iter_source_fix_patches(&self, templated_file: &TemplatedFile) -> Vec<FixPatch> {
        let source_fixes = self.get_source_fixes();
        let mut patches = Vec::with_capacity(source_fixes.len());

        for source_fix in &source_fixes {
            patches.push(FixPatch::new(
                source_fix.templated_slice.clone(),
                source_fix.edit.clone(),
                // String::from("source"),
                source_fix.source_slice.clone(),
                templated_file.templated_str.clone().unwrap()[source_fix.templated_slice.clone()]
                    .to_string(),
                templated_file.source_str[source_fix.source_slice.clone()].to_string(),
            ));
        }

        patches
    }

    pub(crate) fn id(&self) -> u32 {
        self.value.id
    }

    /// Return any source fixes as list.
    pub(crate) fn get_source_fixes(&self) -> Vec<SourceFix> {
        match &self.value.kind {
            NodeOrTokenKind::Node(node) => node.source_fixes.clone(),
            NodeOrTokenKind::Token(_) => Vec::new(),
        }
    }

    pub(crate) fn edit(
        &self,
        id: u32,
        raw: Option<String>,
        _source_fixes: Option<Vec<SourceFix>>,
    ) -> ErasedSegment {
        match &self.value.kind {
            NodeOrTokenKind::Node(_node) => {
                todo!()
            }
            NodeOrTokenKind::Token(token) => {
                let raw = raw.as_deref().unwrap_or(token.raw.as_ref());
                SegmentBuilder::token(id, raw, self.value.syntax_kind)
                    .with_position(self.get_position_marker().unwrap().clone())
                    .finish()
            }
        }
    }

    pub(crate) fn class_types(&self) -> &SyntaxSet {
        &self.value.class_types
    }

    pub(crate) fn first_non_whitespace_segment_raw_upper(&self) -> Option<String> {
        for seg in self.get_raw_segments() {
            if !seg.raw().is_empty() {
                return Some(seg.get_raw_upper().unwrap());
            }
        }
        None
    }

    pub(crate) fn is(&self, other: &ErasedSegment) -> bool {
        Rc::ptr_eq(&self.value, &other.value)
    }

    pub(crate) fn addr(&self) -> usize {
        fn addr<T: ?Sized>(t: *const T) -> usize {
            let c: *const () = t.cast();
            sptr::Strict::addr(c)
        }

        addr(Rc::as_ptr(&self.value))
    }

    pub(crate) fn direct_descendant_type_set(&self) -> SyntaxSet {
        self.segments().iter().fold(SyntaxSet::EMPTY, |set, it| set.union(it.class_types()))
    }

    pub(crate) fn is_keyword(&self, p0: &str) -> bool {
        self.is_type(SyntaxKind::Keyword) && self.raw().eq_ignore_ascii_case(p0)
    }

    pub(crate) fn hash_value(&self) -> u64 {
        let mut hash = self.hash.load(Ordering::Acquire);

        if hash == 0 {
            let mut hasher = ahash::AHasher::default();
            self.get_type().hash(&mut hasher);
            self.raw().hash(&mut hasher);

            if let Some(marker) = &self.get_position_marker() {
                marker.source_position().hash(&mut hasher);
            } else {
                None::<usize>.hash(&mut hasher);
            }

            hash = hasher.finish();

            let exchange = self.hash.compare_exchange(0, hash, Ordering::AcqRel, Ordering::Acquire);
            if let Err(old) = exchange {
                hash = old
            }
        }

        hash
    }

    pub(crate) fn deep_clone(&self) -> Self {
        Self { value: Rc::new(self.value.as_ref().clone()), hash: self.hash.clone() }
    }

    #[track_caller]
    pub(crate) fn get_mut(&mut self) -> &mut NodeOrToken {
        Rc::get_mut(&mut self.value).unwrap()
    }

    #[track_caller]
    pub(crate) fn make_mut(&mut self) -> &mut NodeOrToken {
        let mut this = self.deep_clone();
        std::mem::swap(self, &mut this);
        Rc::get_mut(&mut self.value).unwrap()
    }

    pub(crate) fn reference(&self) -> ObjectReferenceSegment {
        ObjectReferenceSegment(
            self.clone(),
            match self.get_type() {
                SyntaxKind::TableReference => ObjectReferenceKind::Table,
                SyntaxKind::WildcardIdentifier => ObjectReferenceKind::WildcardIdentifier,
                _ => ObjectReferenceKind::Object,
            },
        )
    }

    pub(crate) fn recursive_crawl_all(&self, reverse: bool) -> Vec<ErasedSegment> {
        let mut result = Vec::with_capacity(self.segments().len() + 1);

        if reverse {
            for seg in self.segments().iter().rev() {
                result.append(&mut seg.recursive_crawl_all(reverse));
            }
            result.push(self.clone());
        } else {
            result.push(self.clone());
            for seg in self.segments() {
                result.append(&mut seg.recursive_crawl_all(reverse));
            }
        }

        result
    }

    pub(crate) fn raw_segments_with_ancestors(&self) -> &[(ErasedSegment, Vec<PathStep>)] {
        match &self.value.kind {
            NodeOrTokenKind::Node(node) => node.raw_segments_with_ancestors.get_or_init(|| {
                let mut buffer: Vec<(ErasedSegment, Vec<PathStep>)> =
                    Vec::with_capacity(self.segments().len());
                let code_idxs: Rc<[usize]> = self.code_indices().into();

                for (idx, seg) in self.segments().iter().enumerate() {
                    let new_step = vec![PathStep {
                        segment: self.clone(),
                        idx,
                        len: self.segments().len(),
                        code_idxs: code_idxs.clone(),
                    }];

                    // Use seg.get_segments().is_empty() as a workaround to check if the segment is
                    // a SyntaxKind::Raw type. In the original Python code, this was achieved
                    // using seg.is_type(SyntaxKind::Raw). Here, we assume that a SyntaxKind::Raw
                    // segment is characterized by having no sub-segments.

                    if seg.segments().is_empty() {
                        buffer.push((seg.clone(), new_step));
                    } else {
                        let extended =
                            seg.raw_segments_with_ancestors().iter().map(|(raw_seg, stack)| {
                                let mut new_step = new_step.clone();
                                new_step.extend_from_slice(stack);
                                (raw_seg.clone(), new_step)
                            });

                        buffer.extend(extended);
                    }
                }

                buffer
            }),
            NodeOrTokenKind::Token(_) => &[],
        }
    }

    pub(crate) fn path_to(&self, other: &ErasedSegment) -> Vec<PathStep> {
        let midpoint = other;

        for (idx, seg) in enumerate(self.segments()) {
            let mut steps = vec![PathStep {
                segment: self.clone(),
                idx,
                len: self.segments().len(),
                code_idxs: self.code_indices().into(),
            }];

            if seg.eq(midpoint) {
                return steps;
            }

            let res = seg.path_to(midpoint);

            if !res.is_empty() {
                steps.extend(res);
                return steps;
            }
        }

        Vec::new()
    }

    pub(crate) fn apply_fixes(
        &self,
        fixes: &mut FxHashMap<u32, AnchorEditInfo>,
    ) -> (ErasedSegment, Vec<ErasedSegment>, Vec<ErasedSegment>, bool) {
        if fixes.is_empty() || self.segments().is_empty() {
            return (self.clone(), Vec::new(), Vec::new(), true);
        }

        let mut seg_buffer = Vec::new();
        let mut fixes_applied = Vec::new();
        let mut _requires_validate = false;

        for seg in self.gather_segments() {
            // Look for uuid match.
            // This handles potential positioning ambiguity.

            let Some(mut anchor_info) = fixes.remove(&seg.id()) else {
                seg_buffer.push(seg.clone());
                continue;
            };

            if anchor_info.fixes.len() == 2
                && anchor_info.fixes[0].edit_type == EditType::CreateAfter
            {
                anchor_info.fixes.reverse();
            }

            let fixes_count = anchor_info.fixes.len();
            for mut f in anchor_info.fixes {
                fixes_applied.push(f.clone());

                // Deletes are easy.
                #[allow(unused_assignments)]
                if f.edit_type == EditType::Delete {
                    // We're just getting rid of this segment.
                    _requires_validate = true;
                    // NOTE: We don't add the segment in this case.
                    continue;
                }

                // Otherwise it must be a replace or a create.
                assert!(matches!(
                    f.edit_type,
                    EditType::Replace | EditType::CreateBefore | EditType::CreateAfter
                ));

                if f.edit_type == EditType::CreateAfter && fixes_count == 1 {
                    // In the case of a creation after that is not part
                    // of a create_before/create_after pair, also add
                    // this segment before the edit.
                    seg_buffer.push(seg.clone());
                }

                let mut consumed_pos = false;
                for s in std::mem::take(f.edit.as_mut().unwrap()) {
                    let mut s = s.deep_clone();
                    if f.edit_type == EditType::Replace && !consumed_pos && s.raw() == seg.raw() {
                        consumed_pos = true;
                        s.get_mut().set_position_marker(seg.get_position_marker().cloned());
                    }

                    seg_buffer.push(s);
                }

                #[allow(unused_assignments)]
                if !(f.edit_type == EditType::Replace
                    && f.edit.as_ref().map_or(false, |x| x.len() == 1)
                    && f.edit.as_ref().unwrap()[0].class_types() == seg.class_types())
                {
                    _requires_validate = true;
                }

                if f.edit_type == EditType::CreateBefore {
                    seg_buffer.push(seg.clone());
                }
            }
        }

        if !fixes_applied.is_empty() {
            seg_buffer =
                position_segments(&seg_buffer, self.get_position_marker().as_ref().unwrap());
        }

        let seg_queue = seg_buffer;
        let mut seg_buffer = Vec::new();
        for seg in seg_queue {
            let (s, pre, post, validated) = seg.apply_fixes(fixes);

            seg_buffer.extend(pre);
            seg_buffer.push(s);
            seg_buffer.extend(post);

            #[allow(unused_assignments)]
            if !validated {
                _requires_validate = true;
            }
        }

        let seg_buffer =
            position_segments(&seg_buffer, self.get_position_marker().as_ref().unwrap());
        (self.new(seg_buffer), Vec::new(), Vec::new(), false)
    }
}

#[cfg(any(test, feature = "serde"))]
pub mod serde {
    use serde::ser::SerializeMap;
    use serde::{Deserialize, Serialize};

    use crate::core::parser::segments::base::ErasedSegment;

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum SerialisedSegmentValue {
        Single(String),
        Nested(Vec<TupleSerialisedSegment>),
    }

    #[derive(Deserialize)]
    pub struct TupleSerialisedSegment(String, SerialisedSegmentValue);

    impl Serialize for TupleSerialisedSegment {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut map = serializer.serialize_map(None)?;
            map.serialize_key(&self.0)?;
            map.serialize_value(&self.1)?;
            map.end()
        }
    }

    impl TupleSerialisedSegment {
        pub fn sinlge(key: String, value: String) -> Self {
            Self(key, SerialisedSegmentValue::Single(value))
        }

        pub fn nested(key: String, segments: Vec<TupleSerialisedSegment>) -> Self {
            Self(key, SerialisedSegmentValue::Nested(segments))
        }
    }

    impl ErasedSegment {
        pub fn to_serialised(&self, code_only: bool, show_raw: bool) -> TupleSerialisedSegment {
            if show_raw && self.segments().is_empty() {
                TupleSerialisedSegment::sinlge(
                    self.get_type().as_str().to_string(),
                    self.raw().to_string(),
                )
            } else if code_only {
                let segments = self
                    .segments()
                    .iter()
                    .filter(|seg| seg.is_code() && !seg.is_meta())
                    .map(|seg| seg.to_serialised(code_only, show_raw))
                    .collect::<Vec<_>>();

                TupleSerialisedSegment::nested(self.get_type().as_str().to_string(), segments)
            } else {
                let segments = self
                    .segments()
                    .iter()
                    .map(|seg| seg.to_serialised(code_only, show_raw))
                    .collect::<Vec<_>>();

                TupleSerialisedSegment::nested(self.get_type().as_str().to_string(), segments)
            }
        }
    }
}

impl PartialEq for ErasedSegment {
    fn eq(&self, other: &Self) -> bool {
        if self.id() == other.id() {
            return true;
        }

        let pos_self = self.get_position_marker();
        let pos_other = other.get_position_marker();
        if let Some((pos_self, pos_other)) = pos_self.zip(pos_other) {
            self.get_type() == other.get_type()
                && pos_self.working_loc() == pos_other.working_loc()
                && self.raw() == other.raw()
        } else {
            false
        }
    }
}

pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub fn position_segments(
    segments: &[ErasedSegment],
    parent_pos: &PositionMarker,
) -> Vec<ErasedSegment> {
    if segments.is_empty() {
        return Vec::new();
    }

    let (mut line_no, mut line_pos) = { (parent_pos.working_line_no, parent_pos.working_line_pos) };

    let mut segment_buffer: Vec<ErasedSegment> = Vec::new();
    for (idx, segment) in enumerate(segments) {
        let old_position = segment.get_position_marker();

        let mut new_position = match old_position {
            Some(pos_marker) => pos_marker.clone(),
            None => {
                let start_point = if idx > 0 {
                    let prev_seg = segment_buffer[idx - 1].clone();
                    Some(prev_seg.get_position_marker().unwrap().end_point_marker())
                } else {
                    Some(parent_pos.start_point_marker())
                };

                let mut end_point = None;
                for fwd_seg in &segments[idx + 1..] {
                    if fwd_seg.get_position_marker().is_some() {
                        end_point = Some(
                            fwd_seg.get_raw_segments()[0]
                                .get_position_marker()
                                .unwrap()
                                .start_point_marker(),
                        );
                        break;
                    }
                }

                if let Some((start_point, end_point)) = start_point.as_ref().zip(end_point.as_ref())
                    && start_point != end_point
                {
                    PositionMarker::from_points(start_point, end_point)
                } else if let Some(start_point) = start_point.as_ref() {
                    start_point.clone()
                } else if let Some(end_point) = end_point.as_ref() {
                    end_point.clone()
                } else {
                    unimplemented!("Unable to position new segment")
                }
            }
        };

        new_position = new_position.with_working_position(line_no, line_pos);
        (line_no, line_pos) =
            PositionMarker::infer_next_position(&segment.raw(), line_no, line_pos);

        let mut new_seg = if !segment.segments().is_empty() && old_position != Some(&new_position) {
            let child_segments = position_segments(segment.segments(), &new_position);
            segment.change_segments(child_segments)
        } else {
            segment.deep_clone()
        };

        new_seg.get_mut().set_position_marker(new_position.into());
        segment_buffer.push(new_seg);
    }

    segment_buffer
}

#[derive(Debug, Clone)]
pub struct NodeOrToken {
    id: u32,
    syntax_kind: SyntaxKind,
    class_types: SyntaxSet,
    position_marker: Option<PositionMarker>,
    kind: NodeOrTokenKind,
}

#[derive(Debug, Clone)]
pub enum NodeOrTokenKind {
    Node(NodeData),
    Token(TokenData),
}

impl NodeOrToken {
    pub fn set_position_marker(&mut self, position_marker: Option<PositionMarker>) {
        self.position_marker = position_marker;
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }
}

#[derive(Debug, Clone)]
pub struct NodeData {
    dialect: DialectKind,
    segments: Vec<ErasedSegment>,
    raw: OnceCell<String>,
    source_fixes: Vec<SourceFix>,
    descendant_type_set: OnceCell<SyntaxSet>,
    raw_segments_with_ancestors: OnceCell<Vec<(ErasedSegment, Vec<PathStep>)>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenData {
    raw: SmolStr,
}

#[track_caller]
pub fn pos_marker(segments: &[ErasedSegment]) -> PositionMarker {
    let markers = segments.iter().flat_map(|seg| seg.get_position_marker());

    PositionMarker::from_child_markers(markers)
}

#[derive(Debug, Clone)]
pub struct PathStep {
    pub segment: ErasedSegment,
    pub idx: usize,
    pub len: usize,
    pub code_idxs: Rc<[usize]>,
}

fn class_types(syntax_kind: SyntaxKind) -> SyntaxSet {
    match syntax_kind {
        SyntaxKind::ColumnReference => SyntaxSet::new(&[SyntaxKind::ObjectReference, syntax_kind]),
        SyntaxKind::WildcardIdentifier => {
            SyntaxSet::new(&[SyntaxKind::WildcardIdentifier, SyntaxKind::ObjectReference])
        }
        SyntaxKind::TableReference => SyntaxSet::new(&[SyntaxKind::ObjectReference, syntax_kind]),
        _ => SyntaxSet::single(syntax_kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::linter::core::compute_anchor_edit_info;
    use crate::core::parser::segments::base::Tables;
    use crate::core::parser::segments::test_functions::{raw_seg, raw_segments};
    use crate::core::rules::base::LintFix;

    #[test]
    /// Test comparison of raw segments.
    fn test_parser_base_segments_raw_compare() {
        let template: TemplatedFile = "foobar".into();
        let rs1 = SegmentBuilder::token(0, "foobar", SyntaxKind::Word)
            .with_position(PositionMarker::new(0..6, 0..6, template.clone(), None, None))
            .finish();
        let rs2 = SegmentBuilder::token(0, "foobar", SyntaxKind::Word)
            .with_position(PositionMarker::new(0..6, 0..6, template.clone(), None, None))
            .finish();

        assert_eq!(rs1, rs2)
    }

    #[test]
    // TODO Implement
    /// Test raw segments behave as expected.
    fn test_parser_base_segments_raw() {
        let raw_seg = raw_seg();

        assert_eq!(raw_seg.raw(), "foobar");
    }

    #[test]
    /// Test BaseSegment.compute_anchor_edit_info().
    fn test_parser_base_segments_compute_anchor_edit_info() {
        let raw_segs = raw_segments();
        let tables = Tables::default();

        // Construct a fix buffer, intentionally with:
        // - one duplicate.
        // - two different incompatible fixes on the same segment.
        let fixes = vec![
            LintFix::replace(
                raw_segs[0].clone(),
                vec![raw_segs[0].edit(tables.next_id(), Some("a".to_string()), None)],
                None,
            ),
            LintFix::replace(
                raw_segs[0].clone(),
                vec![raw_segs[0].edit(tables.next_id(), Some("a".to_string()), None)],
                None,
            ),
            LintFix::replace(
                raw_segs[0].clone(),
                vec![raw_segs[0].edit(tables.next_id(), Some("b".to_string()), None)],
                None,
            ),
        ];

        let anchor_edit_info = compute_anchor_edit_info(fixes);

        // Check the target segment is the only key we have.
        assert_eq!(anchor_edit_info.keys().collect::<Vec<_>>(), vec![&raw_segs[0].id()]);

        let anchor_info = anchor_edit_info.get(&raw_segs[0].id()).unwrap();

        // Check that the duplicate as been deduplicated i.e. this isn't 3.
        assert_eq!(anchor_info.replace, 2);

        // Check the fixes themselves.
        //   Note: There's no duplicated first fix.
        assert_eq!(
            anchor_info.fixes[0],
            LintFix::replace(
                raw_segs[0].clone(),
                vec![raw_segs[0].edit(tables.next_id(), Some("a".to_string()), None)],
                None,
            )
        );
        assert_eq!(
            anchor_info.fixes[1],
            LintFix::replace(
                raw_segs[0].clone(),
                vec![raw_segs[0].edit(tables.next_id(), Some("b".to_string()), None)],
                None,
            )
        );

        // Check the first replace
        assert_eq!(
            anchor_info.first_replace,
            Some(LintFix::replace(
                raw_segs[0].clone(),
                vec![raw_segs[0].edit(tables.next_id(), Some("a".to_string()), None)],
                None,
            ))
        );
    }
}

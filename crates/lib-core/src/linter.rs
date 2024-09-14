use ahash::HashMapExt;
use rustc_hash::FxHashMap;

use crate::rules::LintFix;
use crate::segments::AnchorEditInfo;

pub fn compute_anchor_edit_info(fixes: Vec<LintFix>) -> FxHashMap<u32, AnchorEditInfo> {
    let mut anchor_info = FxHashMap::new();

    for fix in fixes {
        let anchor_id = fix.anchor.id();
        anchor_info.entry(anchor_id).or_insert_with(AnchorEditInfo::default).add(fix);
    }

    anchor_info
}

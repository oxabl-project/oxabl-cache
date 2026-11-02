//! The only paths that turn a segment dead, with the accounting that must follow.

use crate::domain::segment::SegmentKind;
use crate::error::Result;
use crate::ids::{CacheId, SegmentId};
use crate::ports::MetaTx;

/// Marks a live segment dead and subtracts its length from its pack and its
/// cache. A pack left with no live bytes becomes dead too. Returns the bytes
/// released, zero if the segment was already dead or unknown.
pub(crate) fn kill_segment(
    tx: &mut dyn MetaTx,
    cache: &CacheId,
    id: &SegmentId,
    now: i64,
) -> Result<u64> {
    let Some(segment) = tx.kill_segment(cache, id, now)? else {
        return Ok(0);
    };
    let length = i64::from(segment.length);
    tx.adjust_cache_bytes(cache, -length, 0)?;
    if tx.adjust_pack_live(cache, &segment.pack, -length)? == 0 {
        tx.mark_pack_dead(cache, &segment.pack, now)?;
    }
    Ok(u64::from(segment.length))
}

/// Marks a dictionary dead once neither the pointer nor any entry names it.
pub(crate) fn release_dictionary(
    tx: &mut dyn MetaTx,
    cache: &CacheId,
    id: &SegmentId,
    now: i64,
) -> Result<u64> {
    let pointed = tx
        .dictionary_pointer(cache)?
        .is_some_and(|pointer| pointer.segment_id == *id);
    if pointed || tx.dictionary_in_use(cache, id)? {
        return Ok(0);
    }
    match tx.segment(cache, id)? {
        Some(segment) if segment.kind == SegmentKind::Dictionary => {
            kill_segment(tx, cache, id, now)
        }
        _ => Ok(0),
    }
}

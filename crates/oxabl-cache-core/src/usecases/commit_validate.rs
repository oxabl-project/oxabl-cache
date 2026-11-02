//! Structural checks of a commit frame, before the pack or the store is touched.

use crate::domain::entry::EntrySpec;
use crate::domain::namespace::NamespaceKind;
use crate::domain::segment::{SegmentKind, SegmentSpec};
use crate::error::{Error, Result};
use crate::limits::{COMMIT_ENTRIES, PACK_HEADER_BYTES, PACK_TRAILER_BYTES, SEGMENTS_PER_PACK};

/// Rejects a frame whose counts exceed the limits, whose segments leave the
/// pack body or overlap, or that names a dictionary outside the trusted namespace.
pub(crate) fn validate_frame(
    kind: NamespaceKind,
    pack_size: u64,
    segments: &[SegmentSpec],
    entries: &[EntrySpec],
) -> Result<()> {
    if segments.len() > SEGMENTS_PER_PACK || entries.len() > COMMIT_ENTRIES {
        return Err(Error::TooLarge);
    }
    let body_end = pack_size.saturating_sub(PACK_TRAILER_BYTES);
    let mut spans: Vec<(u64, u64)> = Vec::with_capacity(segments.len());
    for segment in segments {
        let end = segment.offset.saturating_add(u64::from(segment.length));
        if segment.length == 0 || segment.offset < PACK_HEADER_BYTES || end > body_end {
            return Err(Error::bad_request("segment outside the pack body"));
        }
        if segment.kind == SegmentKind::Dictionary && kind != NamespaceKind::Trusted {
            return Err(Error::bad_request("a dictionary segment is trusted only"));
        }
        spans.push((segment.offset, end));
    }
    spans.sort_unstable();
    if spans.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(Error::bad_request("segments overlap"));
    }
    Ok(())
}

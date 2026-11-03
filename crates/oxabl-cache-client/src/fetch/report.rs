use oxabl_cache_format::{PackId, SegmentId};

use crate::cache::Inner;

const MAX_REPORTS: usize = 10;

/// Tells the server about a segment that does not hash to its id, in the background and at
/// most once per segment and ten times per handle.
pub(crate) fn hash_mismatch(inner: &Inner, pack: PackId, segment: SegmentId) {
    {
        let mut reported = inner.reported.lock().expect("report lock");
        if reported.len() >= MAX_REPORTS || !reported.insert(segment) {
            return;
        }
    }
    let api = inner.api.clone();
    inner.handle.spawn(async move {
        if let Err(error) = api.report_hash_mismatch(pack, segment).await {
            tracing::debug!(?error, "the report was not accepted");
        }
    });
}

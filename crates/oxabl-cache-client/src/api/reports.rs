use oxabl_cache_format::{PackId, SegmentId};

use super::{Api, ApiError};
use crate::progress::Progress;

impl Api {
    /// Tells the server that a segment's bytes do not hash to its id.
    pub async fn report_hash_mismatch(
        &self,
        pack: PackId,
        segment: SegmentId,
    ) -> Result<(), ApiError> {
        let body = serde_json::json!({
            "pack_id": pack.to_hex(),
            "segment_id": segment.to_hex(),
            "kind": "hash_mismatch",
        });
        self.call(&Progress::new(), |c| {
            c.post(self.url("/reports")).json(&body)
        })
        .await
        .map(|_| ())
    }
}

//! Sealed telemetry reports.

use super::{PgTx, RowExt, args, int};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::telemetry::StoredTelemetry;
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::metadata::TelemetryTx;

impl TelemetryTx for PgTx<'_> {
    fn append_telemetry(
        &mut self,
        cache: &CacheId,
        received_at: i64,
        body: &[u8],
        keep: usize,
        oldest: i64,
    ) -> Result<u64> {
        let id = cache.as_bytes().as_slice();
        let seq = self
            .query_one(
                "INSERT INTO telemetry (cache_id, seq, received_at, body)
                 SELECT $1, COALESCE(MAX(seq), 0) + 1, $2, $3 FROM telemetry WHERE cache_id = $1
                 RETURNING seq",
                args![id, received_at, body],
            )?
            .int(0)?;
        self.execute(
            "DELETE FROM telemetry WHERE cache_id = $1 AND (seq <= $2 OR received_at < $3)",
            args![id, seq - int(keep as u64), oldest],
        )?;
        Ok(u64::try_from(seq).unwrap_or(0))
    }

    fn telemetry_after(
        &self,
        cache: &CacheId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredTelemetry>> {
        self.query(
            "SELECT seq, received_at, body FROM telemetry
             WHERE cache_id = $1 AND seq > $2 ORDER BY seq LIMIT $3",
            args![cache.as_bytes().as_slice(), int(after), int(limit as u64)],
        )?
        .iter()
        .map(|row| {
            Ok(StoredTelemetry {
                seq: row.unsigned(0)?,
                received_at: row.int(1)?,
                body: row.bytes(2)?,
            })
        })
        .collect()
    }
}

//! Sealed telemetry reports.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, int};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::telemetry::StoredTelemetry;
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::metadata::TelemetryTx;
use rusqlite::params;

impl TelemetryTx for SqliteTx<'_> {
    fn append_telemetry(
        &mut self,
        cache: &CacheId,
        received_at: i64,
        body: &[u8],
        keep: usize,
        oldest: i64,
    ) -> Result<u64> {
        let id = cache.as_bytes().as_slice();
        let seq: i64 = self
            .conn
            .prepare_cached("SELECT COALESCE(MAX(seq), 0) + 1 FROM telemetry WHERE cache_id = ?1")
            .db()?
            .query_row([id], |row| row.get(0))
            .db()?;
        self.conn
            .prepare_cached(
                "INSERT INTO telemetry (cache_id, seq, received_at, body) VALUES (?1, ?2, ?3, ?4)",
            )
            .db()?
            .execute(params![id, seq, received_at, body])
            .db()?;
        self.conn
            .prepare_cached(
                "DELETE FROM telemetry WHERE cache_id = ?1 AND (seq <= ?2 OR received_at < ?3)",
            )
            .db()?
            .execute(params![id, seq - int(keep as u64), oldest])
            .db()?;
        Ok(u64::try_from(seq).unwrap_or(0))
    }

    fn telemetry_after(
        &self,
        cache: &CacheId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredTelemetry>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT seq, received_at, body FROM telemetry
                 WHERE cache_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
            )
            .db()?;
        let rows = statement
            .query_map(
                params![cache.as_bytes().as_slice(), int(after), int(limit as u64)],
                |row| {
                    Ok(StoredTelemetry {
                        seq: row.unsigned(0)?,
                        received_at: row.get(1)?,
                        body: row.get(2)?,
                    })
                },
            )
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }
}

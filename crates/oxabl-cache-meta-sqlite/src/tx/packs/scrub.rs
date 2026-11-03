//! The scrub queue.

use crate::db::{DbResult, RowExt, int};
use crate::tx::SqliteTx;
use oxabl_cache_core::Result;
use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::metadata::ScrubQueueTx;
use rusqlite::params;

impl ScrubQueueTx for SqliteTx<'_> {
    fn queue_scrub(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO scrub_queue (cache_id, pack_id, queued_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (cache_id, pack_id) DO NOTHING",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                now
            ])
            .db()?;
        Ok(())
    }

    fn take_scrubs(&mut self, limit: usize) -> Result<Vec<(CacheId, PackId)>> {
        let queued: Vec<(CacheId, PackId)> = {
            let mut statement = self
                .conn
                .prepare_cached(
                    "SELECT cache_id, pack_id FROM scrub_queue ORDER BY queued_at LIMIT ?1",
                )
                .db()?;
            let rows = statement
                .query_map([int(limit as u64)], |row| {
                    Ok((
                        CacheId::from_bytes(row.blob(0)?),
                        PackId::from_bytes(row.blob(1)?),
                    ))
                })
                .db()?;
            rows.collect::<rusqlite::Result<_>>().db()?
        };
        for (cache, pack) in &queued {
            self.conn
                .execute(
                    "DELETE FROM scrub_queue WHERE cache_id = ?1 AND pack_id = ?2",
                    [cache.as_bytes().as_slice(), pack.as_bytes().as_slice()],
                )
                .db()?;
        }
        Ok(queued)
    }
}

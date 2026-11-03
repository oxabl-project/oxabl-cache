//! The scrub queue.

use crate::tx::{PgTx, RowExt, args, int};
use oxabl_cache_core::Result;
use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::metadata::ScrubQueueTx;

impl ScrubQueueTx for PgTx<'_> {
    fn queue_scrub(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<()> {
        self.execute(
            "INSERT INTO scrub_queue (cache_id, pack_id, queued_at) VALUES ($1, $2, $3)
             ON CONFLICT (cache_id, pack_id) DO NOTHING",
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice(), now],
        )?;
        Ok(())
    }

    fn take_scrubs(&mut self, limit: usize) -> Result<Vec<(CacheId, PackId)>> {
        self.query(
            "DELETE FROM scrub_queue WHERE (cache_id, pack_id) IN
               (SELECT cache_id, pack_id FROM scrub_queue ORDER BY queued_at LIMIT $1)
             RETURNING cache_id, pack_id",
            args![int(limit as u64)],
        )?
        .iter()
        .map(|row| {
            Ok((
                CacheId::from_bytes(row.blob(0)?),
                PackId::from_bytes(row.blob(1)?),
            ))
        })
        .collect()
    }
}

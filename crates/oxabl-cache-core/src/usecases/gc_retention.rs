//! Retention: entries not read within the cache's retention period are deleted.

use crate::engine::Engine;
use crate::error::Result;
use crate::ports::TxScope;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::time::day_of;
use crate::usecases::entry_removal::remove_entries;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Deletes entries whose access day is older than their cache's retention and
    /// returns how many. Bytes follow through the mark rules, grace and sweep.
    pub async fn gc_retention(&self) -> Result<u64> {
        let caches = self.meta.read(|tx| tx.all_caches()).await?;
        let batch = self.config.gc_batch;
        let mut total = 0;
        for cache in caches {
            let cutoff = day_of(self.now()) - i64::from(cache.retention_days);
            let id = cache.id;
            loop {
                let now = self.now();
                let removed = self
                    .meta
                    .transact_in(TxScope::Cache(id), move |tx| {
                        let idle = tx.entries_idle_before(&id, cutoff, batch)?;
                        Ok(remove_entries(tx, &id, &idle, now, |_| false)?.entries)
                    })
                    .await?;
                if removed == 0 {
                    break;
                }
                total += removed;
            }
        }
        Ok(total)
    }
}

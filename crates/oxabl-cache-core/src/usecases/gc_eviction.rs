//! Quota eviction on live bytes: personal entries first, oldest access day first.

use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ports::TxScope;
use crate::ports::metadata::EvictionScope;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::entry_removal::remove_entries;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// For each cache above the high watermark of its hard quota, deletes entries
    /// until live bytes are at or below the low watermark. Returns the entries deleted.
    pub async fn gc_eviction(&self) -> Result<u64> {
        let caches = self.meta.read(|tx| tx.all_caches()).await?;
        let (high, low) = (
            self.config.evict_high_permille,
            self.config.evict_low_permille,
        );
        let batch = self.config.gc_batch;
        let mut total = 0;
        for cache in caches {
            if cache.live_bytes.saturating_mul(1000) <= cache.hard_bytes.saturating_mul(high) {
                continue;
            }
            let target = cache.hard_bytes / 1000 * low;
            let (tenant, id) = (cache.tenant, cache.id);
            for scope in [EvictionScope::Personal, EvictionScope::Trusted] {
                loop {
                    let now = self.now();
                    let removed = self
                        .meta
                        .transact_in(TxScope::Cache(id), move |tx| {
                            let current = tx.cache(&tenant, &id)?.ok_or(Error::NotFound)?;
                            if current.live_bytes <= target {
                                return Ok(0);
                            }
                            let excess = current.live_bytes - target;
                            let oldest = tx.oldest_entries(&id, scope, batch)?;
                            let removal =
                                remove_entries(tx, &id, &oldest, now, |freed| freed >= excess)?;
                            Ok(removal.entries)
                        })
                        .await?;
                    if removed == 0 {
                        break;
                    }
                    total += removed;
                }
            }
        }
        Ok(total)
    }
}

//! Sweep: delete the bytes and rows of packs that are dead past the grace or expired.

use crate::domain::pack::PackState;
use crate::engine::Engine;
use crate::error::Result;
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler};

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Moves due packs to `deleting`, deletes their objects, then their rows.
    /// Returns the packs removed. A pack is due when it has been dead for the
    /// grace, or is pending with an expired lease, or an earlier sweep stopped
    /// halfway.
    pub async fn gc_sweep(&self) -> Result<u64> {
        let grace = self.config.grace_secs;
        let batch = self.config.gc_batch;
        let mut removed = 0;
        loop {
            let now = self.now();
            let due = self
                .meta
                .transact(move |tx| {
                    let due = tx.packs_due_for_sweep(now, grace, batch)?;
                    for pack in &due {
                        if tx.set_pack_deleting(&pack.cache, &pack.id)? == Some(PackState::Pending)
                        {
                            let pending = i64::try_from(pack.size).unwrap_or(i64::MAX);
                            tx.adjust_cache_bytes(&pack.cache, 0, -pending)?;
                        }
                    }
                    Ok(due)
                })
                .await?;
            if due.is_empty() {
                return Ok(removed);
            }
            for pack in due {
                let key = ObjectKey {
                    cache: pack.cache,
                    pack: pack.id,
                };
                self.store.delete(&key).await?;
                self.meta
                    .transact(move |tx| {
                        tx.delete_dead_segments_of_pack(&pack.cache, &pack.id)?;
                        tx.delete_pack(&pack.cache, &pack.id)
                    })
                    .await?;
                removed += 1;
            }
        }
    }
}

//! Orphan scan: objects no pack row names.

use crate::engine::Engine;
use crate::error::Result;
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler};
use std::collections::HashSet;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Deletes objects without a pack row that are older than twice the lease.
    /// Returns how many. Covers a lost database and a crash between deleting an
    /// object and its row.
    pub async fn gc_orphans(&self) -> Result<u64> {
        let caches = self.meta.read(|tx| tx.all_caches()).await?;
        let mut deleted = 0;
        for cache in caches {
            let id = cache.id;
            let known: HashSet<_> = self
                .meta
                .read(move |tx| tx.pack_ids(&id))
                .await?
                .into_iter()
                .collect();
            let cutoff = self.now() - 2 * self.config.lease_secs;
            for object in self.store.list(&id).await? {
                if !known.contains(&object.pack) && object.modified_at < cutoff {
                    self.require_gc_leadership().await?;
                    self.store
                        .delete(&ObjectKey {
                            cache: id,
                            pack: object.pack,
                        })
                        .await?;
                    deleted += 1;
                }
            }
        }
        Ok(deleted)
    }
}

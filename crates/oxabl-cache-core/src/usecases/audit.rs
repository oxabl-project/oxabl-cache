//! Auditing that the byte counters equal the sums of live segment lengths.

use crate::engine::Engine;
use crate::error::Result;
use crate::ids::CacheId;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Recomputes live bytes from segment rows and describes every disagreement
    /// with the stored counters of the cache and its packs. Empty means consistent.
    pub async fn audit_cache(&self, cache: CacheId) -> Result<Vec<String>> {
        self.meta
            .transact(move |tx| tx.audit_live_bytes(&cache))
            .await
    }
}

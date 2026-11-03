//! Scrubbing: re-verifying packs on request or from the queue.

use crate::engine::Engine;
use crate::error::Result;
use crate::ids::CacheId;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::pack_verify::VerifyOutcome;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Verifies the packs queued by a changed ETag; returns the combined outcome.
    pub async fn run_queued_scrubs(&self) -> Result<VerifyOutcome> {
        let batch = self.config.gc_batch;
        let queued = self.meta.transact(move |tx| tx.take_scrubs(batch)).await?;
        let mut total = VerifyOutcome::default();
        for (cache, pack) in queued {
            let outcome = self.verify_pack(cache, pack).await?;
            total.corrupt_segments += outcome.corrupt_segments;
            total.entries_removed += outcome.entries_removed;
        }
        Ok(total)
    }

    /// Verifies every pack of a cache now.
    pub async fn scrub_cache(&self, cache: CacheId) -> Result<VerifyOutcome> {
        let packs = self.meta.read(move |tx| tx.pack_ids(&cache)).await?;
        let mut total = VerifyOutcome::default();
        for pack in packs {
            let outcome = self.verify_pack(cache, pack).await?;
            total.corrupt_segments += outcome.corrupt_segments;
            total.entries_removed += outcome.entries_removed;
        }
        Ok(total)
    }
}

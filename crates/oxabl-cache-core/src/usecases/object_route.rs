//! Checks and accounting for the local-disk object route.

use crate::domain::pack::PackState;
use crate::domain::usage::Metric;
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::{CacheId, PackId};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Whether a signed PUT may write the pack now: it must be pending, never
    /// committed, and `content_length` must equal the size it was reserved with.
    /// `NotFound` for an unknown pack, `Conflict(PackClosed)` once it is closed,
    /// `BadRequest` for a different size.
    pub async fn pack_accepts_put(
        &self,
        cache: CacheId,
        pack: PackId,
        content_length: u64,
    ) -> Result<()> {
        let found = self
            .meta
            .transact(move |tx| tx.pack(&cache, &pack))
            .await?
            .ok_or(Error::NotFound)?;
        if found.state != PackState::Pending || found.commit_digest.is_some() {
            return Err(Error::Conflict(Conflict::PackClosed));
        }
        if found.size != content_length {
            return Err(Error::bad_request(
                "content length differs from the reserved size",
            ));
        }
        Ok(())
    }

    /// Whether a signed GET may read the pack: it must be a live pack. Anything
    /// else is `NotFound`.
    pub async fn pack_serves_get(&self, cache: CacheId, pack: PackId) -> Result<()> {
        match self.meta.transact(move |tx| tx.pack(&cache, &pack)).await? {
            Some(found) if found.state == PackState::Live => Ok(()),
            _ => Err(Error::NotFound),
        }
    }

    /// Counts bytes the object route served, exactly, against the cache's tenant.
    pub async fn count_served_bytes(&self, cache: CacheId, bytes: u64) -> Result<()> {
        let tenant = self
            .meta
            .transact(move |tx| tx.cache_tenant(&cache))
            .await?
            .ok_or(Error::NotFound)?;
        self.count(tenant, cache, Metric::EgressBytes, bytes);
        Ok(())
    }
}

//! Opening a cache: identity, epoch, versions, limits and byte accounting.

use crate::domain::cache::CacheQuota;
use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::Result;
use crate::ids::CacheId;
use crate::limits::{COMMIT_ENTRIES, LOOKUP_IDS, PACK_BYTES};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler, StoreCapabilities};
use crate::usecases::scoped_cache::scoped_cache;

/// The limits a client must respect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Lookup ids per request.
    pub lookup_ids: usize,
    /// Entries per commit.
    pub commit_entries: usize,
    /// Largest pack in bytes.
    pub pack_bytes: u64,
}

/// What `GET /v1/caches/{cache}` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheInfo {
    /// The cache id.
    pub id: CacheId,
    /// Customer-chosen name.
    pub display_name: String,
    /// Generation of the cache root key.
    pub epoch: u32,
    /// Version of the current roster.
    pub roster_version: u64,
    /// Version of the dictionary pointer, 0 for none.
    pub dictionary_version: u64,
    /// What the object store supports.
    pub store: StoreCapabilities,
    /// Request limits.
    pub limits: Limits,
    /// Byte accounting.
    pub quota: CacheQuota,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Describes a cache. Needs `read`.
    pub async fn cache_info(&self, caller: &Caller, cache: CacheId) -> Result<CacheInfo> {
        let caller = caller.clone();
        let (found, dictionary_version, totals) = self
            .meta
            .transact(move |tx| {
                let found = scoped_cache(tx, &caller, &cache, Scope::Read)?;
                let dictionary_version = tx.dictionary_pointer(&cache)?.map_or(0, |p| p.version);
                let totals = tx.pack_totals(&cache)?;
                Ok((found, dictionary_version, totals))
            })
            .await?;
        Ok(CacheInfo {
            id: found.id,
            display_name: found.display_name,
            epoch: found.epoch,
            roster_version: found.roster_version,
            dictionary_version,
            store: self.store.capabilities(),
            limits: Limits {
                lookup_ids: LOOKUP_IDS,
                commit_entries: COMMIT_ENTRIES,
                pack_bytes: PACK_BYTES,
            },
            quota: CacheQuota {
                hard_bytes: found.hard_bytes,
                live_bytes: found.live_bytes,
                pending_bytes: found.pending_bytes,
                stored_bytes: totals.stored_bytes,
                reclaimable_bytes: totals.reclaimable_bytes,
            },
        })
    }
}

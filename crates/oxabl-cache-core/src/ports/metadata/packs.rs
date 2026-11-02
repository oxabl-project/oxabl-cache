//! Packs, their accounting and the scrub queue.

use crate::domain::namespace::NamespaceKey;
use crate::domain::pack::{Pack, PackState, PackTotals};
use crate::error::Result;
use crate::ids::{CacheId, ClientRef, PackId, PrincipalId};

/// Queries and writes of pack rows.
pub trait PacksTx {
    /// A pack of a cache.
    fn pack(&self, cache: &CacheId, id: &PackId) -> Result<Option<Pack>>;
    /// The pack an uploader reserved under a retry key.
    fn pack_by_client_ref(
        &self,
        cache: &CacheId,
        uploader: &PrincipalId,
        client_ref: &ClientRef,
    ) -> Result<Option<Pack>>;
    /// Inserts a pack row.
    fn insert_pack(&mut self, pack: &Pack) -> Result<()>;
    /// Byte totals over the cache's packs.
    fn pack_totals(&self, cache: &CacheId) -> Result<PackTotals>;
    /// Live bytes plus pending sizes of one namespace.
    fn namespace_bytes(&self, cache: &CacheId, namespace: &NamespaceKey) -> Result<u64>;
    /// Flips a pending pack to live and records the commit. Returns `false`,
    /// changing nothing, if the pack is not pending.
    fn record_commit(
        &mut self,
        cache: &CacheId,
        id: &PackId,
        commit: &RecordedCommit<'_>,
    ) -> Result<bool>;
    /// Replaces the ETag recorded for a committed pack.
    fn set_pack_etag(&mut self, cache: &CacheId, id: &PackId, etag: &str) -> Result<()>;
    /// Adds a signed delta to a pack's live bytes and returns the new value; a
    /// value below zero or above the pack size is an internal error.
    fn adjust_pack_live(&mut self, cache: &CacheId, id: &PackId, delta: i64) -> Result<u64>;
    /// Sets `dead_since` on a live pack that has none; `false` otherwise.
    fn mark_pack_dead(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<bool>;
    /// Moves a pending or live pack to `Deleting` and returns its previous state;
    /// `None` if it was already deleting or is gone.
    fn set_pack_deleting(&mut self, cache: &CacheId, id: &PackId) -> Result<Option<PackState>>;
    /// Deletes a pack row; fails if segment rows still reference it.
    fn delete_pack(&mut self, cache: &CacheId, id: &PackId) -> Result<bool>;
    /// Packs ready for the sweep: dead longer than the grace, pending with an
    /// expired lease, or already deleting.
    fn packs_due_for_sweep(&self, now: i64, grace: i64, limit: usize) -> Result<Vec<Pack>>;
    /// Live packs created before `created_before` whose live share is below
    /// `max_live_permille` thousandths, at least `min_size` bytes large, with live bytes left.
    fn compaction_candidates(
        &self,
        created_before: i64,
        max_live_permille: u64,
        min_size: u64,
        limit: usize,
    ) -> Result<Vec<Pack>>;
    /// Live packs whose live bytes are zero but that carry no `dead_since`.
    fn empty_unmarked_packs(&self, limit: usize) -> Result<Vec<Pack>>;
    /// Every pack id of a cache.
    fn pack_ids(&self, cache: &CacheId) -> Result<Vec<PackId>>;
    /// Recomputes live-byte sums from segment rows and describes each mismatch
    /// with the stored counters of the cache and of its packs.
    fn audit_live_bytes(&self, cache: &CacheId) -> Result<Vec<String>>;
    /// Queues a pack for a full re-verification.
    fn queue_scrub(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<()>;
    /// Removes and returns up to `limit` queued scrubs.
    fn take_scrubs(&mut self, limit: usize) -> Result<Vec<(CacheId, PackId)>>;
}

/// What a commit records on its pack.
#[derive(Debug, Clone, Copy)]
pub struct RecordedCommit<'a> {
    /// The object's ETag at commit.
    pub etag: &'a str,
    /// Live bytes credited to the pack.
    pub live_bytes: u64,
    /// BLAKE3 of the request body.
    pub digest: [u8; 32],
    /// The per-entry statuses returned.
    pub statuses: &'a [u8],
    /// Set when the pack is dead at birth.
    pub dead_since: Option<i64>,
}

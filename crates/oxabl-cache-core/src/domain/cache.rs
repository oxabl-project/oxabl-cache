//! Caches and their quotas.

use crate::ids::{CacheId, TenantId};

/// One cache per codebase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    /// The cache id.
    pub id: CacheId,
    /// The owning tenant.
    pub tenant: TenantId,
    /// Customer-chosen name.
    pub display_name: String,
    /// Days an entry may go unread before retention deletes it.
    pub retention_days: u32,
    /// Hard limit on live plus pending bytes.
    pub hard_bytes: u64,
    /// Generation of the cache root key, from the current roster.
    pub epoch: u32,
    /// Version of the current roster, 0 before the first.
    pub roster_version: u64,
    /// Sum of the lengths of live segments.
    pub live_bytes: u64,
    /// Sum of the sizes of pending packs.
    pub pending_bytes: u64,
    /// Creation time, unix seconds.
    pub created_at: i64,
}

/// Byte accounting of one cache, as reported to its readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheQuota {
    /// The hard limit.
    pub hard_bytes: u64,
    /// Bytes entries still name; drives eviction and admission.
    pub live_bytes: u64,
    /// Bytes of packs awaiting commit.
    pub pending_bytes: u64,
    /// Bytes occupying storage: pending, live and in grace.
    pub stored_bytes: u64,
    /// Stored bytes that compaction and the sweep will free.
    pub reclaimable_bytes: u64,
}

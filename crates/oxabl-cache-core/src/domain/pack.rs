//! Packs.

use crate::domain::namespace::NamespaceKey;
use crate::ids::{CacheId, ClientRef, PackId, PrincipalId};

/// Lifecycle of a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackState {
    /// Reserved; bytes may be uploading.
    Pending,
    /// Committed.
    Live,
    /// Selected for deletion.
    Deleting,
}

/// An immutable object of segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    /// The cache the pack belongs to.
    pub cache: CacheId,
    /// The pack id.
    pub id: PackId,
    /// The namespace of every segment in the pack.
    pub namespace: NamespaceKey,
    /// The principal that reserved the pack; zero for packs written by GC.
    pub uploader: PrincipalId,
    /// The client's retry key.
    pub client_ref: ClientRef,
    /// Declared size in bytes.
    pub size: u64,
    /// Object ETag recorded at commit.
    pub etag: Option<String>,
    /// Sum of the lengths of live segments.
    pub live_bytes: u64,
    /// Lifecycle state.
    pub state: PackState,
    /// Lease end while pending, unix seconds.
    pub lease_until: i64,
    /// Creation time, unix seconds.
    pub created_at: i64,
    /// When the pack lost its last live segment.
    pub dead_since: Option<i64>,
    /// BLAKE3 of the commit request body.
    pub commit_digest: Option<[u8; 32]>,
    /// The per-entry statuses returned by the commit.
    pub commit_statuses: Option<Vec<u8>>,
}

/// Byte totals of the packs of one cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PackTotals {
    /// Sizes of all packs, pending, live and deleting.
    pub stored_bytes: u64,
    /// Sizes of pending packs.
    pub pending_bytes: u64,
    /// Live bytes of all packs.
    pub live_bytes: u64,
}

//! Byte storage for packs.

use crate::error::Result;
use crate::ids::{CacheId, PackId};
use std::future::Future;

/// The address of a pack object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectKey {
    /// The cache the pack belongs to.
    pub cache: CacheId,
    /// The pack.
    pub pack: PackId,
}

/// What a store supports beyond the portable subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreCapabilities {
    /// `fs` or `s3`.
    pub kind: &'static str,
    /// `If-None-Match: *` is honoured on PUT.
    pub if_none_match: bool,
    /// `x-amz-checksum-sha256` is verified on PUT.
    pub checksum_sha256: bool,
}

/// A time-limited capability to move one pack's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedUrl {
    /// The URL to call.
    pub url: String,
    /// `PUT` or `GET`.
    pub method: &'static str,
    /// Headers the call must carry exactly.
    pub headers: Vec<(String, String)>,
    /// When the URL stops working, unix seconds.
    pub expires_at: i64,
}

/// Size and ETag of a stored object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectHead {
    /// Size in bytes.
    pub size: u64,
    /// Entity tag; changes if the bytes change.
    pub etag: String,
}

/// An object found by a listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectInfo {
    /// The pack the object is named after.
    pub pack: PackId,
    /// Size in bytes.
    pub size: u64,
    /// Last modification, unix seconds.
    pub modified_at: i64,
}

/// A byte store for immutable pack objects.
pub trait ObjectStore: Send + Sync + 'static {
    /// What the store supports.
    fn capabilities(&self) -> StoreCapabilities;

    /// Issues a PUT URL for an object of exactly `size` bytes.
    fn put_url(
        &self,
        key: &ObjectKey,
        size: u64,
        sha256: Option<[u8; 32]>,
        expires_at: i64,
    ) -> impl Future<Output = Result<SignedUrl>> + Send;

    /// Issues a GET URL that serves any range of the object.
    fn get_url(
        &self,
        key: &ObjectKey,
        expires_at: i64,
    ) -> impl Future<Output = Result<SignedUrl>> + Send;

    /// Size and ETag, or `None` if the object does not exist.
    fn head(&self, key: &ObjectKey) -> impl Future<Output = Result<Option<ObjectHead>>> + Send;

    /// Reads `length` bytes at `offset`. With `if_match`, a different ETag is
    /// `Conflict(PackChanged)`; a missing object is `NotFound`.
    fn read_range(
        &self,
        key: &ObjectKey,
        offset: u64,
        length: u64,
        if_match: Option<&str>,
    ) -> impl Future<Output = Result<Vec<u8>>> + Send;

    /// Writes a whole object durably and returns its head.
    fn put(
        &self,
        key: &ObjectKey,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<ObjectHead>> + Send;

    /// Deletes an object; deleting a missing object succeeds.
    fn delete(&self, key: &ObjectKey) -> impl Future<Output = Result<()>> + Send;

    /// Lists the objects under a cache prefix.
    fn list(&self, cache: &CacheId) -> impl Future<Output = Result<Vec<ObjectInfo>>> + Send;

    /// Fails unless the store can serve and take objects right now. Writes no user data.
    fn check(&self) -> impl Future<Output = Result<()>> + Send {
        async {
            let probe = ObjectKey {
                cache: CacheId::random(),
                pack: PackId::random(),
            };
            self.head(&probe).await.map(|_| ())
        }
    }
}

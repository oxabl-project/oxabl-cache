//! The `ObjectStore` port on S3.

use crate::signing::Window;
use crate::store::S3Store;
use oxabl_cache_core::Result;
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{
    ObjectHead, ObjectInfo, ObjectKey, ObjectStore, SignedUrl, StoreCapabilities,
};

impl ObjectStore for S3Store {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            kind: "s3",
            if_none_match: self.optional.if_none_match,
            checksum_sha256: self.optional.checksum_sha256,
        }
    }

    async fn put_url(
        &self,
        key: &ObjectKey,
        size: u64,
        sha256: Option<[u8; 32]>,
        expires_at: i64,
    ) -> Result<SignedUrl> {
        let digest = sha256.filter(|_| self.optional.checksum_sha256);
        Ok(self.put_signed(
            &self.layout.object(key),
            size,
            digest.as_ref(),
            self.optional.if_none_match,
            &Window::until(expires_at),
        ))
    }

    async fn get_url(&self, key: &ObjectKey, expires_at: i64) -> Result<SignedUrl> {
        Ok(self.get_signed(&self.layout.object(key), &Window::until(expires_at)))
    }

    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectHead>> {
        self.head_key(&self.layout.object(key)).await
    }

    async fn read_range(
        &self,
        key: &ObjectKey,
        offset: u64,
        length: u64,
        if_match: Option<&str>,
    ) -> Result<Vec<u8>> {
        self.read_key_range(&self.layout.object(key), offset, length, if_match)
            .await
    }

    async fn put(&self, key: &ObjectKey, bytes: Vec<u8>) -> Result<ObjectHead> {
        self.put_key(&self.layout.object(key), bytes).await
    }

    async fn delete(&self, key: &ObjectKey) -> Result<()> {
        self.delete_keys(&[self.layout.object(key)]).await
    }

    async fn list(&self, cache: &CacheId) -> Result<Vec<ObjectInfo>> {
        self.list_cache(cache).await
    }
}

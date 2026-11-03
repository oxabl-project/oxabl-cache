//! The configured object store: a local directory or an S3 bucket.

use crate::config::{Config, StoreConfig};
use crate::keys;
use oxabl_cache_core::Result;
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{
    ObjectHead, ObjectInfo, ObjectKey, ObjectStore, SignedUrl, StoreCapabilities,
};
use oxabl_cache_store_fs::FsStore;
use oxabl_cache_store_s3::S3Store;

/// Either store, chosen by `store.kind`.
pub(crate) enum AnyStore {
    Fs(FsStore),
    S3(S3Store),
}

/// Opens the store of `config`. The local-disk store is also returned by itself: the
/// object route serves its objects. An S3 store runs its conformance probe first.
pub(crate) async fn open(
    config: &Config,
    public_url: &str,
) -> std::result::Result<(AnyStore, Option<FsStore>), String> {
    match &config.store {
        StoreConfig::Fs { root } => {
            let url_keys = keys::read_url_keys(&config.url_key_file)?;
            let store = FsStore::open(root.clone(), public_url, url_keys)
                .map_err(|error| format!("cannot open the object store: {error}"))?;
            Ok((AnyStore::Fs(store.clone()), Some(store)))
        }
        StoreConfig::S3(s3) => {
            let store = S3Store::open(s3)
                .await
                .map_err(|error| format!("cannot open the object store: {error}"))?;
            Ok((AnyStore::S3(store), None))
        }
    }
}

/// Opens the store of `config` for reading only: nothing is created, removed or written.
pub(crate) fn open_for_reading(config: &Config) -> std::result::Result<AnyStore, String> {
    let opened = match &config.store {
        StoreConfig::Fs { root } => {
            let url_keys = keys::read_url_keys(&config.url_key_file)?;
            FsStore::open_existing(root.clone(), "http://localhost", url_keys).map(AnyStore::Fs)
        }
        StoreConfig::S3(s3) => S3Store::open_unprobed(s3).map(AnyStore::S3),
    };
    opened.map_err(|error| format!("cannot open the object store: {error}"))
}

/// Whether two configurations name the same directory or the same bucket and prefix.
pub(crate) fn same_store(a: &StoreConfig, b: &StoreConfig) -> bool {
    match (a, b) {
        (StoreConfig::Fs { root: a }, StoreConfig::Fs { root: b }) => {
            match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
                (Ok(a), Ok(b)) => a == b,
                _ => a == b,
            }
        }
        (StoreConfig::S3(a), StoreConfig::S3(b)) => {
            a.endpoint == b.endpoint && a.bucket == b.bucket && a.prefix == b.prefix
        }
        _ => false,
    }
}

impl ObjectStore for AnyStore {
    fn capabilities(&self) -> StoreCapabilities {
        match self {
            Self::Fs(store) => store.capabilities(),
            Self::S3(store) => store.capabilities(),
        }
    }

    async fn put_url(
        &self,
        key: &ObjectKey,
        size: u64,
        sha256: Option<[u8; 32]>,
        expires_at: i64,
    ) -> Result<SignedUrl> {
        match self {
            Self::Fs(store) => store.put_url(key, size, sha256, expires_at).await,
            Self::S3(store) => store.put_url(key, size, sha256, expires_at).await,
        }
    }

    async fn get_url(&self, key: &ObjectKey, expires_at: i64) -> Result<SignedUrl> {
        match self {
            Self::Fs(store) => store.get_url(key, expires_at).await,
            Self::S3(store) => store.get_url(key, expires_at).await,
        }
    }

    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectHead>> {
        match self {
            Self::Fs(store) => store.head(key).await,
            Self::S3(store) => store.head(key).await,
        }
    }

    async fn read_range(
        &self,
        key: &ObjectKey,
        offset: u64,
        length: u64,
        if_match: Option<&str>,
    ) -> Result<Vec<u8>> {
        match self {
            Self::Fs(store) => store.read_range(key, offset, length, if_match).await,
            Self::S3(store) => store.read_range(key, offset, length, if_match).await,
        }
    }

    async fn put(&self, key: &ObjectKey, bytes: Vec<u8>) -> Result<ObjectHead> {
        match self {
            Self::Fs(store) => store.put(key, bytes).await,
            Self::S3(store) => store.put(key, bytes).await,
        }
    }

    async fn delete(&self, key: &ObjectKey) -> Result<()> {
        match self {
            Self::Fs(store) => store.delete(key).await,
            Self::S3(store) => store.delete(key).await,
        }
    }

    async fn list(&self, cache: &CacheId) -> Result<Vec<ObjectInfo>> {
        match self {
            Self::Fs(store) => store.list(cache).await,
            Self::S3(store) => store.list(cache).await,
        }
    }

    async fn check(&self) -> Result<()> {
        match self {
            Self::Fs(store) => store.check().await,
            Self::S3(store) => store.check().await,
        }
    }
}

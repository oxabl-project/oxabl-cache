//! The `ObjectStore` port on a directory.

use crate::error::io_error;
use crate::head::{head_of, open_with_head};
use crate::layout::{cache_dir, object_path, pack_of_name};
use crate::store::FsStore;
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{
    ObjectHead, ObjectInfo, ObjectKey, ObjectStore, SignedUrl, StoreCapabilities,
};
use oxabl_cache_core::{Conflict, Error, Result};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::time::UNIX_EPOCH;

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::internal(format!("blocking task: {e}")))?
}

impl ObjectStore for FsStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            kind: "fs",
            if_none_match: false,
            checksum_sha256: false,
        }
    }

    async fn put_url(
        &self,
        key: &ObjectKey,
        size: u64,
        _sha256: Option<[u8; 32]>,
        expires_at: i64,
    ) -> Result<SignedUrl> {
        Ok(SignedUrl {
            url: self.signed_url("PUT", key, expires_at, Some(size)),
            method: "PUT",
            headers: vec![("content-length".to_owned(), size.to_string())],
            expires_at,
        })
    }

    async fn get_url(&self, key: &ObjectKey, expires_at: i64) -> Result<SignedUrl> {
        Ok(SignedUrl {
            url: self.signed_url("GET", key, expires_at, None),
            method: "GET",
            headers: Vec::new(),
            expires_at,
        })
    }

    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectHead>> {
        let path = object_path(&self.root, key);
        blocking(move || match head_of(&path) {
            Ok(head) => Ok(Some(head)),
            Err(error) => match io_error(&error) {
                Error::NotFound => Ok(None),
                other => Err(other),
            },
        })
        .await
    }

    async fn read_range(
        &self,
        key: &ObjectKey,
        offset: u64,
        length: u64,
        if_match: Option<&str>,
    ) -> Result<Vec<u8>> {
        let path = object_path(&self.root, key);
        let if_match = if_match.map(str::to_owned);
        blocking(move || {
            let (mut file, head) = open_with_head(&path).map_err(|e| io_error(&e))?;
            if if_match.is_some_and(|etag| etag != head.etag) {
                return Err(Error::Conflict(Conflict::PackChanged));
            }
            if offset.checked_add(length).is_none_or(|end| end > head.size) {
                return Err(Error::bad_request("range outside the object"));
            }
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| io_error(&e))?;
            let mut bytes = vec![0u8; usize::try_from(length).map_err(|_| Error::TooLarge)?];
            file.read_exact(&mut bytes).map_err(|e| io_error(&e))?;
            Ok(bytes)
        })
        .await
    }

    async fn put(&self, key: &ObjectKey, bytes: Vec<u8>) -> Result<ObjectHead> {
        let store = self.clone();
        let key = *key;
        blocking(move || {
            let mut writer = store.writer(&key)?;
            writer.write_all(&bytes)?;
            writer.finish(None)
        })
        .await
    }

    async fn delete(&self, key: &ObjectKey) -> Result<()> {
        let path = object_path(&self.root, key);
        blocking(move || match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) => match io_error(&error) {
                Error::NotFound => Ok(()),
                other => Err(other),
            },
        })
        .await
    }

    async fn list(&self, cache: &CacheId) -> Result<Vec<ObjectInfo>> {
        let directory = cache_dir(&self.root, cache);
        blocking(move || {
            let mut found = Vec::new();
            let shards = match fs::read_dir(&directory) {
                Ok(shards) => shards,
                Err(error) => {
                    return match io_error(&error) {
                        Error::NotFound => Ok(found),
                        other => Err(other),
                    };
                }
            };
            for shard in shards {
                let shard = shard.map_err(|e| io_error(&e))?;
                for file in fs::read_dir(shard.path()).map_err(|e| io_error(&e))? {
                    let file = file.map_err(|e| io_error(&e))?;
                    let Some(pack) = file.file_name().to_str().and_then(pack_of_name) else {
                        continue;
                    };
                    let meta = file.metadata().map_err(|e| io_error(&e))?;
                    let modified = meta
                        .modified()
                        .map_err(|e| io_error(&e))?
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |elapsed| {
                            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
                        });
                    found.push(ObjectInfo {
                        pack,
                        size: meta.len(),
                        modified_at: modified,
                    });
                }
            }
            Ok(found)
        })
        .await
    }

    async fn check(&self) -> Result<()> {
        let store = self.clone();
        blocking(move || store.check_writable()).await
    }
}

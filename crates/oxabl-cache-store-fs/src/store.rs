//! The store handle: root directory and URL keys.

use crate::error::io_error;
use crate::layout::tmp_dir;
use crate::url_signing::{UrlRejection, object_path, sign, verify};
use crate::writer::ObjectWriter;
use oxabl_cache_core::Result;
use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::ObjectKey;
use std::fs;
use std::path::PathBuf;

/// The parts of a request to the object route that its signature covers.
#[derive(Debug, Clone, Copy)]
pub struct UrlRequest<'a> {
    /// `GET` or `PUT`.
    pub method: &'a str,
    /// The cache in the path.
    pub cache: CacheId,
    /// The pack in the path.
    pub pack: PackId,
    /// The `exp` query value.
    pub expires_at: i64,
    /// The request's `content-length`, absent for a GET.
    pub content_length: Option<u64>,
    /// The `sig` query value.
    pub signature: &'a str,
}

/// An object store on a local directory. Cloning is cheap; clones share the directory.
#[derive(Debug, Clone)]
pub struct FsStore {
    pub(crate) root: PathBuf,
    pub(crate) public_url: String,
    pub(crate) url_keys: Vec<[u8; 32]>,
}

impl FsStore {
    /// Opens a store at `root`, creating its directories and discarding temporary
    /// files a crash left behind. `public_url` is the origin that serves the
    /// object route; `url_keys` lists the HMAC keys, the first of which signs and
    /// all of which verify (two during rotation).
    pub fn open(root: PathBuf, public_url: &str, url_keys: Vec<[u8; 32]>) -> Result<Self> {
        assert!(
            !url_keys.is_empty(),
            "an fs store needs at least one URL key"
        );
        fs::create_dir_all(root.join("v1")).map_err(|e| io_error(&e))?;
        let tmp = tmp_dir(&root);
        fs::create_dir_all(&tmp).map_err(|e| io_error(&e))?;
        for leftover in fs::read_dir(&tmp).map_err(|e| io_error(&e))? {
            let leftover = leftover.map_err(|e| io_error(&e))?;
            fs::remove_file(leftover.path()).map_err(|e| io_error(&e))?;
        }
        Ok(Self {
            root,
            public_url: public_url.trim_end_matches('/').to_owned(),
            url_keys,
        })
    }

    /// Starts a durable write of one object; used by the object PUT route.
    pub fn writer(&self, key: &ObjectKey) -> Result<ObjectWriter> {
        ObjectWriter::create(&self.root, key)
    }

    /// Checks the signature of an object URL the route received.
    pub fn verify_url(
        &self,
        request: &UrlRequest<'_>,
        now: i64,
    ) -> std::result::Result<(), UrlRejection> {
        verify(
            &self.url_keys,
            request.method,
            &object_path(&request.cache, &request.pack),
            request.expires_at,
            request.content_length,
            request.signature,
            now,
        )
    }

    pub(crate) fn signed_url(
        &self,
        method: &str,
        key: &ObjectKey,
        expires_at: i64,
        content_length: Option<u64>,
    ) -> String {
        let path = object_path(&key.cache, &key.pack);
        let signature = sign(&self.url_keys[0], method, &path, expires_at, content_length);
        format!("{}{path}?exp={expires_at}&sig={signature}", self.public_url)
    }
}

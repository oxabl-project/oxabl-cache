//! The object store under test: a local directory, or a bucket on Garage or versitygw.

use oxabl_cache_e2e::garage::Garage;
use oxabl_cache_e2e::s3::S3Bucket;
use oxabl_cache_e2e::versitygw::Versitygw;
use std::fs;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Which store a test target runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fs,
    Garage,
    Versitygw,
}

pub fn block_on<T>(work: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(work)
}

/// A store with its own empty bucket or directory.
#[derive(Clone)]
pub enum Objects {
    Fs { root: PathBuf },
    S3 { bucket: S3Bucket },
}

fn secret_file(path: &Path, text: &str) {
    fs::write(path, text).expect("credential file");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("mode");
}

/// The `[store]` section of a server configuration for `bucket`, with its
/// credentials written under `dir`.
pub fn s3_section(dir: &Path, bucket: &S3Bucket) -> String {
    let (key_id, secret) = (dir.join("s3-key-id"), dir.join("s3-secret"));
    secret_file(&key_id, &bucket.access_key);
    secret_file(&secret, &bucket.secret_key);
    format!(
        "[store]\nkind = \"s3\"\nendpoint = \"{}\"\nregion = \"{}\"\nbucket = \"{}\"\n\
         path_style = true\naccess_key_id_file = \"{}\"\nsecret_access_key_file = \"{}\"\n",
        bucket.endpoint,
        bucket.region,
        bucket.bucket,
        key_id.display(),
        secret.display(),
    )
}

impl Objects {
    /// A fresh store of `kind` for a server whose data directory is `dir`.
    pub fn open(kind: Kind, dir: &Path) -> Self {
        match kind {
            Kind::Fs => Self::Fs {
                root: dir.join("objects"),
            },
            Kind::Garage => Self::S3 {
                bucket: block_on(Garage::shared().new_bucket()).expect("garage bucket"),
            },
            Kind::Versitygw => Self::S3 {
                bucket: block_on(Versitygw::shared().new_bucket()).expect("versitygw bucket"),
            },
        }
    }

    /// The `[store]` section of the server configuration.
    pub fn section(&self, dir: &Path) -> String {
        match self {
            Self::Fs { root } => format!("[store]\nkind = \"fs\"\nroot = \"{}\"\n", root.display()),
            Self::S3 { bucket } => s3_section(dir, bucket),
        }
    }

    fn fs_path(root: &Path, cache: &str, pack: &str) -> PathBuf {
        root.join("v1").join(cache).join(&pack[..2]).join(pack)
    }

    fn key(cache: &str, pack: &str) -> String {
        format!("v1/{cache}/{pack}")
    }

    pub fn exists(&self, cache: &str, pack: &str) -> bool {
        match self {
            Self::Fs { root } => Self::fs_path(root, cache, pack).exists(),
            Self::S3 { bucket } => block_on(bucket.head(&Self::key(cache, pack)))
                .expect("head")
                .is_some(),
        }
    }

    /// How many of `packs` are stored. On a bucket it is one listing, so the count is a single
    /// snapshot even while the server deletes.
    pub fn present(&self, cache: &str, packs: &[String]) -> usize {
        match self {
            Self::Fs { .. } => packs.iter().filter(|pack| self.exists(cache, pack)).count(),
            Self::S3 { bucket } => {
                let keys = block_on(bucket.list(&format!("v1/{cache}/"))).expect("list");
                packs
                    .iter()
                    .filter(|pack| keys.contains(&Self::key(cache, pack)))
                    .count()
            }
        }
    }

    pub fn read(&self, cache: &str, pack: &str) -> Vec<u8> {
        match self {
            Self::Fs { root } => fs::read(Self::fs_path(root, cache, pack)).expect("object"),
            Self::S3 { bucket } => {
                let key = Self::key(cache, pack);
                block_on(async {
                    let size = bucket.head(&key).await.expect("head").expect("object");
                    bucket.get_range(&key, 0, size - 1).await.expect("get")
                })
            }
        }
    }

    /// Replaces an object's bytes behind the server's back.
    pub fn overwrite(&self, cache: &str, pack: &str, bytes: Vec<u8>) {
        match self {
            Self::Fs { root } => {
                fs::write(Self::fs_path(root, cache, pack), bytes).expect("overwrite");
            }
            Self::S3 { bucket } => {
                block_on(bucket.put(&Self::key(cache, pack), bytes)).expect("overwrite");
            }
        }
    }
}

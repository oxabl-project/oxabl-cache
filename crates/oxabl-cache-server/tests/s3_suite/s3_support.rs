//! Helpers for scenarios that reach the bucket behind the server's back.

use crate::support::fixture::World;
use crate::support::objects::{Objects, block_on};
use oxabl_cache_e2e::s3::S3Bucket;

impl Objects {
    pub fn bucket(&self) -> &S3Bucket {
        match self {
            Self::S3 { bucket } => bucket,
            Self::Fs { .. } => panic!("an S3 scenario ran against a local store"),
        }
    }
}

impl World {
    pub fn bucket(&self) -> &S3Bucket {
        self.server.objects.bucket()
    }

    /// The keys in the bucket under `prefix`.
    pub fn bucket_keys(&self, prefix: &str) -> Vec<String> {
        block_on(self.bucket().list(prefix)).expect("list")
    }
}

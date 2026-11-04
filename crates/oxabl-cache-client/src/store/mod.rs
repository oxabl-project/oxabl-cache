//! Storing: lazy verified reads, sealing, packing, upload and commit.

mod dictionary;
pub(crate) mod pack_build;
mod packer;
pub(crate) mod publish;
mod read_files;
mod seal;
mod shared;
mod types;
mod uploader;
mod validate;

pub use types::{ArtifactFileRef, StoreFailure, StoreOutcome, StoreReport};
pub use uploader::Uploader;

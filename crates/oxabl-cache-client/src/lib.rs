//! Public client library used by oxabl.
//!
//! Everything between a plaintext [`CompileKey`] and a verified file in oxabl's stage:
//! lookup ids, compression, encryption, signing, packing, batching, transfer, verification
//! and atomic installation. Every cache failure is a miss or "not stored"; the client never
//! fails a build.

mod api;
mod artifact_path;
mod auth;
mod cache;
mod compile_key;
mod config;
mod error;
mod fetch;
mod install;
mod mode;
mod progress;
mod roster_refresh;
mod state;
mod stats;
mod store;

pub use artifact_path::{ArtifactPath, PathError};
pub use cache::{Cache, Presence};
pub use compile_key::CompileKey;
pub use config::{Config, ConfigError, Credentials, IdentitySource};
pub use error::{CacheError, IdTokenError, OpenError};
pub use fetch::{
    Fetch, FetchOptions, FetchOutcome, FetchRequest, InstalledFile, MissReason, Provenance, Tier,
};
pub use install::Durability;
pub use mode::Mode;
pub use oxabl_cache_crypto::SecretString;
pub use stats::Stats;
pub use store::{ArtifactFileRef, StoreFailure, StoreOutcome, StoreReport, Uploader};

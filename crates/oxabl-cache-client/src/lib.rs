//! Public client library used by oxabl.
//!
//! Everything between a plaintext [`CompileKey`] and a verified file in oxabl's stage:
//! lookup ids, compression, encryption, signing, packing, batching, transfer, verification
//! and atomic installation. Every cache failure is a miss or "not stored"; the client never
//! fails a build.

mod activity;
mod api;
mod artifact_path;
mod auth;
mod cache;
mod compile_key;
mod config;
mod dictionary;
mod error;
mod error_log;
mod fetch;
mod identity;
mod install;
mod invite;
mod keyring;
mod login;
mod me;
mod mode;
mod owner;
mod progress;
mod roster_refresh;
mod roster_summary;
mod state;
mod stats;
mod status;
mod store;

pub use activity::{Activity, ActivityKind, ActivitySummary};
pub use artifact_path::{ArtifactPath, PathError};
pub use cache::{Cache, MAX_SAMPLE, Presence};
pub use compile_key::CompileKey;
pub use config::{Config, ConfigError, Credentials, IdentitySource};
pub use dictionary::{
    DictionaryError, DictionaryPublished, MIN_PUBLISH_GAIN, TrainError, Trained, TrainingOptions,
    train_from_outputs,
};
pub use error::{CacheError, IdTokenError, OpenError};
pub use error_log::LoggedError;
pub use fetch::{
    Fetch, FetchOptions, FetchOutcome, FetchRequest, InstalledFile, MissReason, Provenance, Tier,
};
pub use identity::{
    Identity, IdentityError, create_identity_file, default_identity_path, open_identity_file,
    signing_key_hex, signing_key_text, write_identity_secret,
};
pub use install::Durability;
pub use invite::Invite;
pub use keyring::{KeyringError, forget_passphrase, save_passphrase, stored_passphrase};
pub use login::{Challenge, DeviceLogin, LoginError, LoginSession, Logout, logout};
pub use me::{Me, VisibleCache, me, round_trip, signed_in};
pub use mode::Mode;
pub use owner::{InitReport, InitRequest, Owner, OwnerError, Published, Purge, init};
pub use oxabl_cache_crypto::{ExposeSecret, SecretString};
pub use oxabl_cache_format::Role;
pub use roster_summary::{MemberSummary, RosterSummary, role_name};
pub use state::default_state_dir;
pub use stats::Stats;
pub use status::{Quota, ServerStatus, Status};
pub use store::{ArtifactFileRef, StoreFailure, StoreOutcome, StoreReport, Uploader};

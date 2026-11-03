//! A handle to one cache.

mod dictionaries;
mod gates;
mod identity;
mod open;
mod probe;
pub(crate) mod roster;
mod view;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};

use oxabl_cache_crypto::Identity;
use oxabl_cache_format::{Limits, Namespaces, PublicKey, SegmentId};
use tokio::runtime::Handle;

use crate::api::{Api, ApiError};
use crate::config::{Config, Pins};
use crate::error::{CacheError, OpenError};
use crate::fetch::{Fetch, FetchOptions, FetchRequest};
use crate::mode::Mode;
use crate::state::HighWater;
use crate::stats::{Counters, Stats};
use crate::store::Uploader;

pub use probe::Presence;

pub(crate) use dictionaries::Dictionaries;
pub(crate) use gates::{Gates, permits_for};
pub(crate) use view::RosterView;

/// Everything the operations of one handle share.
pub(crate) struct Inner {
    pub api: Api,
    pub pins: Pins,
    pub identity: Identity,
    pub reader_key: PublicKey,
    pub mode: Mode,
    pub namespaces: Namespaces,
    pub view: RwLock<Arc<RosterView>>,
    pub high_water: HighWater,
    pub dictionaries: Dictionaries,
    pub limits: Limits,
    pub zstd_level: i32,
    pub gates: Gates,
    pub handle: Handle,
    pub checksum_sha256: bool,
    pub counters: Arc<Counters>,
    pub reported: Mutex<HashSet<SegmentId>>,
}

/// A handle to one cache; cheap to clone and safe to share across tasks.
#[derive(Clone)]
pub struct Cache {
    pub(crate) inner: Arc<Inner>,
}

/// Maps a failed call during `open` to the error `open` reports.
pub(crate) fn open_error(error: ApiError) -> OpenError {
    match error {
        ApiError::Transport => OpenError::Unreachable,
        ApiError::Malformed => OpenError::BadResponse,
        ApiError::Status(e) => match e.status {
            401 | 403 => OpenError::Authentication,
            404 => OpenError::CacheNotFound,
            status => OpenError::Server(status),
        },
    }
}

impl Inner {
    /// The roster and keys in force now.
    pub fn view(&self) -> Arc<RosterView> {
        self.view.read().expect("roster view lock").clone()
    }
}

impl Cache {
    /// Authenticates, fetches and verifies the roster, unwraps the root key.
    pub async fn open(config: Config) -> Result<Cache, OpenError> {
        open::open(config).await
    }

    /// Plan mode: presence only, no verification. A hit predicts; `fetch` decides.
    pub async fn probe(&self, keys: &[crate::CompileKey]) -> Result<Vec<Presence>, CacheError> {
        probe::probe(&self.inner, keys).await
    }

    /// Starts fetching; yields one outcome per request as each completes.
    pub fn fetch(&self, requests: Vec<FetchRequest>, options: FetchOptions) -> Fetch {
        Fetch::start(self.inner.clone(), requests, options)
    }

    /// Starts a batched upload session. Fails if the mode does not write.
    pub fn uploader(&self) -> Result<Uploader, CacheError> {
        Uploader::start(self.inner.clone())
    }

    pub fn mode(&self) -> Mode {
        self.inner.mode
    }

    pub fn stats(&self) -> Stats {
        self.inner.counters.snapshot()
    }
}

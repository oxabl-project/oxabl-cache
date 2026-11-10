//! A handle to one cache.

mod dictionaries;
mod gates;
pub(crate) mod identity;
mod open;
mod probe;
pub(crate) mod roster;
mod view;

use std::collections::HashSet;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex, RwLock};

use oxabl_cache_crypto::Identity;
use oxabl_cache_format::{Limits, Namespaces, PublicKey, SegmentId};
use tokio::runtime::Handle;

use crate::activity::{self, Activity, Journal};
use crate::api::{Api, ApiError};
use crate::config::{Config, Pins};
use crate::error::{CacheError, OpenError};
use crate::error_log::{self, ErrorLog};
use crate::fetch::{Fetch, FetchOptions, FetchRequest};
use crate::mode::Mode;
use crate::progress::Progress;
use crate::state::{HighWater, StateDirs};
use crate::stats::{Counters, Stats};
use crate::store::Uploader;

pub use probe::Presence;

/// The most entries `Cache::fetch_sample` asks for.
pub const MAX_SAMPLE: usize = oxabl_cache_format::limits::SAMPLE_IDS;

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
    pub errors: ErrorLog,
    pub reported: Mutex<HashSet<SegmentId>>,
    pub journal: Journal,
    pub rtt_ms: AtomicU32,
    pub uploading: tokio::sync::Mutex<()>,
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
        let log = StateDirs::new(config.state_dir.as_deref())
            .error_log(&config.pins.tenant.to_text(), &config.pins.cache.to_text());
        let opened = open::open(config).await;
        if let Err(error) = &opened {
            let (kind, message) = error_log::open_failure(error);
            log.record("open", kind, &message);
        }
        opened
    }

    /// The last `limit` failures logged for this cache on this machine, newest first.
    pub fn recent_errors(&self, limit: usize) -> Vec<crate::LoggedError> {
        self.inner.errors.recent(limit)
    }

    /// Plan mode: presence only, no verification. A hit predicts; `fetch` decides.
    pub async fn probe(&self, keys: &[crate::CompileKey]) -> Result<Vec<Presence>, CacheError> {
        let probed = probe::probe(&self.inner, keys).await;
        if let Err(error) = &probed {
            self.inner
                .errors
                .record("probe", error_log::cache_kind(error), &error.to_string());
        }
        probed
    }

    /// Starts fetching; yields one outcome per request as each completes.
    pub fn fetch(&self, requests: Vec<FetchRequest>, options: FetchOptions) -> Fetch {
        Fetch::start(self.inner.clone(), requests, options)
    }

    /// Asks the server for up to `count` live trusted entries (1 to 1,000) and starts fetching
    /// them with every verification `fetch` does. An outcome's `key` holds the entry's lookup
    /// id. An empty cache gives an empty sample, so no outcomes.
    pub async fn fetch_sample(
        &self,
        count: usize,
        options: FetchOptions,
    ) -> Result<Fetch, CacheError> {
        if count == 0 || count > MAX_SAMPLE {
            return Err(CacheError::SampleSize);
        }
        let ids = self
            .inner
            .api
            .sample(count, &Progress::new())
            .await
            .map_err(|_| CacheError::Unavailable)?;
        Ok(Fetch::start_sampled(self.inner.clone(), ids, options))
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

    /// The fetches and stores the cache's members shared in the current epoch, oldest first.
    /// The server keeps them sealed: only members read them.
    pub async fn team_activity(&self) -> Result<Vec<Activity>, CacheError> {
        activity::team_activity(&self.inner).await
    }

    /// Sends the activities of this machine that are not yet shared, and waits until it is done.
    pub async fn share_activity(&self) {
        activity::upload_pending(self.inner.clone()).await;
    }
}

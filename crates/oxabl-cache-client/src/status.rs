//! What a cache handle reports about itself and the server.

use serde::Deserialize;

use crate::cache::Cache;
use crate::error::CacheError;
use crate::mode::Mode;
use crate::progress::Progress;
use crate::roster_summary::{MemberSummary, RosterSummary};

/// The cache's byte accounting, as the server reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Quota {
    pub hard_bytes: u64,
    pub live_bytes: u64,
    pub pending_bytes: u64,
    pub stored_bytes: u64,
    pub reclaimable_bytes: u64,
}

/// What the server says about the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerStatus {
    pub epoch: u32,
    pub roster_version: u64,
    pub dictionary_version: u64,
    pub quota: Quota,
}

/// A handle's mode, the member it acts as, the roster it verified and the server's numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub mode: Mode,
    pub principal: MemberSummary,
    pub roster: RosterSummary,
    /// The oldest roster version the repository accepts.
    pub min_roster_version: u64,
    pub server: ServerStatus,
}

impl Cache {
    /// Reads the server's view of the cache next to the verified roster this handle holds.
    pub async fn status(&self) -> Result<Status, CacheError> {
        let inner = &self.inner;
        let info = inner
            .api
            .info(&Progress::new())
            .await
            .map_err(|_| CacheError::Unavailable)?;
        let view = inner.view();
        let principal = view
            .roster
            .member_of(&inner.identity)
            .map(MemberSummary::of)
            .map_err(|_| CacheError::Unavailable)?;
        Ok(Status {
            mode: inner.mode,
            principal,
            roster: RosterSummary::of(&view.roster),
            min_roster_version: inner.pins.min_roster_version,
            server: ServerStatus {
                epoch: info.epoch,
                roster_version: info.roster_version,
                dictionary_version: info.dictionary_version,
                quota: info.quota,
            },
        })
    }
}

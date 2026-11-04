use serde::Deserialize;

use super::{Api, ApiError};
use crate::progress::Progress;
use crate::status::Quota;

const BODY_CAP: usize = 64 * 1024;

#[derive(Deserialize)]
struct Store {
    #[serde(default)]
    checksum_sha256: bool,
}

/// What `GET /v1/caches/{cache}` says that the client acts on or reports.
#[derive(Deserialize)]
pub(crate) struct CacheInfo {
    store: Store,
    pub epoch: u32,
    pub roster_version: u64,
    pub dictionary_version: u64,
    pub quota: Quota,
}

impl CacheInfo {
    /// Whether the store wants a SHA-256 of every pack at reservation.
    pub fn checksum_sha256(&self) -> bool {
        self.store.checksum_sha256
    }
}

impl Api {
    pub async fn info(&self, progress: &Progress) -> Result<CacheInfo, ApiError> {
        let response = self.call(progress, |c| c.get(self.url(""))).await?;
        let body = super::body::read_capped(response, BODY_CAP, progress, &self.counters).await?;
        serde_json::from_slice(&body).map_err(|_| ApiError::Malformed)
    }
}

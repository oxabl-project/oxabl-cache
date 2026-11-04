use bytes::Bytes;
use oxabl_cache_format::limits::ROSTER_BYTES;
use reqwest::header::CONTENT_TYPE;

use super::{Api, ApiError, BINARY};
use crate::progress::Progress;

impl Api {
    /// The `OXRS` bytes, or `None` while the cache has no roster.
    pub async fn roster(&self, progress: &Progress) -> Result<Option<Vec<u8>>, ApiError> {
        match self.call(progress, |c| c.get(self.url("/roster"))).await {
            Ok(response) => {
                super::body::read_capped(response, ROSTER_BYTES, progress, &self.counters)
                    .await
                    .map(Some)
            }
            Err(e) if e.status() == Some(404) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Publishes `bytes` over the roster of version `current` (0 while there is none).
    pub async fn put_roster(&self, current: u64, bytes: &Bytes) -> Result<(), ApiError> {
        self.counters.sent(bytes.len() as u64);
        self.call(&Progress::new(), |c| {
            c.put(self.url("/roster"))
                .header("If-Match", format!("\"v{current}\""))
                .header(CONTENT_TYPE, BINARY)
                .body(bytes.clone())
        })
        .await
        .map(|_| ())
    }
}

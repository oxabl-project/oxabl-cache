use oxabl_cache_format::limits::ROSTER_BYTES;

use super::{Api, ApiError};
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
}

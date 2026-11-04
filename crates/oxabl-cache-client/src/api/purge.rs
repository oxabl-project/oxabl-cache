use serde::Deserialize;

use super::{Api, ApiError};
use crate::progress::Progress;

const BODY_CAP: usize = 64 * 1024;

#[derive(Deserialize)]
struct Marked {
    entries_marked: u64,
}

impl Api {
    /// Marks every entry of an epoch below `epoch` dead; returns how many it marked.
    pub async fn purge_below(&self, epoch: u32) -> Result<u64, ApiError> {
        let body = serde_json::json!({ "below_epoch": epoch });
        let progress = Progress::new();
        let response = self
            .call(&progress, |c| c.post(self.url("/purge")).json(&body))
            .await?;
        let bytes = super::body::read_capped(response, BODY_CAP, &progress, &self.counters).await?;
        serde_json::from_slice::<Marked>(&bytes)
            .map(|m| m.entries_marked)
            .map_err(|_| ApiError::Malformed)
    }
}

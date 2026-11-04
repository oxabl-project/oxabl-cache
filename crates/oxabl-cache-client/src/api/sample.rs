use oxabl_cache_format::LookupId;
use serde::Deserialize;

use super::{Api, ApiError};
use crate::progress::Progress;

const BODY_CAP: usize = 256 * 1024;

#[derive(Deserialize)]
struct Sample {
    ids: Vec<String>,
}

impl Api {
    /// Up to `count` live lookup ids of the trusted namespace; the answer is checked to hold
    /// no more than `count` distinct ids.
    pub async fn sample(
        &self,
        count: usize,
        progress: &Progress,
    ) -> Result<Vec<LookupId>, ApiError> {
        let url = self.url(&format!("/sample?namespace=trusted&n={count}"));
        let response = self.call(progress, |c| c.get(&url)).await?;
        let body = super::body::read_capped(response, BODY_CAP, progress, &self.counters).await?;
        let sample: Sample = serde_json::from_slice(&body).map_err(|_| ApiError::Malformed)?;
        let mut ids = sample
            .ids
            .iter()
            .map(|id| LookupId::from_hex(id).map_err(|_| ApiError::Malformed))
            .collect::<Result<Vec<_>, _>>()?;
        ids.sort();
        ids.dedup();
        if ids.len() != sample.ids.len() || ids.len() > count {
            return Err(ApiError::Malformed);
        }
        Ok(ids)
    }
}

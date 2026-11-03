use std::collections::HashMap;

use bytes::Bytes;
use oxabl_cache_format::PackId;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;

use super::{Api, ApiError, BINARY};
use crate::progress::Progress;

const LOOKUP_BODY_CAP: usize = 64 * 1024 * 1024;
const URLS_BODY_CAP: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
struct Fresh {
    url: String,
    expires_at: u64,
}

#[derive(Deserialize)]
struct Urls {
    urls: HashMap<String, Fresh>,
}

impl Api {
    /// Posts an `OXLQ` frame and returns the `OXLR` bytes.
    pub async fn lookup(&self, frame: Vec<u8>, progress: &Progress) -> Result<Vec<u8>, ApiError> {
        self.counters.sent(frame.len() as u64);
        let frame = Bytes::from(frame);
        let response = self
            .call(progress, |c| {
                c.post(self.url("/lookup"))
                    .header(CONTENT_TYPE, BINARY)
                    .body(frame.clone())
            })
            .await?;
        super::body::read_capped(response, LOOKUP_BODY_CAP, progress, &self.counters).await
    }

    /// Fresh GET URLs for packs; packs the caller cannot read are omitted.
    pub async fn urls(
        &self,
        packs: &[PackId],
        progress: &Progress,
    ) -> Result<HashMap<PackId, (String, u64)>, ApiError> {
        let body =
            serde_json::json!({ "packs": packs.iter().map(PackId::to_hex).collect::<Vec<_>>() });
        let response = self
            .call(progress, |c| c.post(self.url("/urls")).json(&body))
            .await?;
        let bytes =
            super::body::read_capped(response, URLS_BODY_CAP, progress, &self.counters).await?;
        let urls: Urls = serde_json::from_slice(&bytes).map_err(|_| ApiError::Malformed)?;
        urls.urls
            .into_iter()
            .map(|(id, fresh)| {
                let id = PackId::from_hex(&id).map_err(|_| ApiError::Malformed)?;
                Ok((id, (fresh.url, fresh.expires_at)))
            })
            .collect()
    }
}

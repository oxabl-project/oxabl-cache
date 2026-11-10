use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::Deserialize;

use super::{Api, ApiError, BINARY};
use crate::progress::Progress;

const PAGE_CAP: usize = 2 * 1024 * 1024;
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
struct Report {
    body: String,
}

#[derive(Deserialize)]
struct Page {
    reports: Vec<Report>,
    next: u64,
}

impl Api {
    /// Appends a sealed report to the cache's telemetry log.
    pub async fn send_telemetry(&self, frame: Vec<u8>) -> Result<(), ApiError> {
        self.call(&Progress::new(), |c| {
            c.post(self.url("/telemetry"))
                .header(reqwest::header::CONTENT_TYPE, BINARY)
                .body(frame.clone())
        })
        .await
        .map(|_| ())
    }

    /// One page of sealed reports after `after`, and the cursor of the next page.
    pub async fn telemetry_page(&self, after: u64) -> Result<(Vec<Vec<u8>>, u64), ApiError> {
        let progress = Progress::new();
        let url = self.url(&format!("/telemetry?after={after}"));
        let response = self.call(&progress, |c| c.get(&url)).await?;
        let body = super::body::read_capped(response, PAGE_CAP, &progress, &self.counters).await?;
        let page: Page = serde_json::from_slice(&body).map_err(|_| ApiError::Malformed)?;
        if page.next < after || (page.reports.is_empty() && page.next != after) {
            return Err(ApiError::Malformed);
        }
        let frames = page
            .reports
            .iter()
            .map(|r| {
                base64::engine::general_purpose::STANDARD
                    .decode(&r.body)
                    .map_err(|_| ApiError::Malformed)
            })
            .collect::<Result<_, _>>()?;
        Ok((frames, page.next))
    }

    /// The round trip of the server's unauthenticated health check.
    pub async fn round_trip(&self) -> Result<Duration, ApiError> {
        let started = Instant::now();
        let response = self
            .http
            .get(format!("{}/v1/health", self.server))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .map_err(|_| ApiError::Transport)?;
        if !response.status().is_success() {
            return Err(ApiError::Malformed);
        }
        Ok(started.elapsed())
    }
}

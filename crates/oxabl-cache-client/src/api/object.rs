use std::collections::BTreeMap;

use bytes::Bytes;
use reqwest::StatusCode;
use reqwest::header::{HeaderName, HeaderValue, RANGE};
use tracing::trace;

use super::{Api, ApiError};
use crate::progress::Progress;

fn only_http(url: &str) -> Result<reqwest::Url, ApiError> {
    let url = reqwest::Url::parse(url).map_err(|_| ApiError::Malformed)?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(ApiError::Malformed),
    }
}

fn status_only(status: StatusCode) -> ApiError {
    ApiError::Status(super::StatusError {
        status: status.as_u16(),
        code: None,
        reason: None,
        retry_after: None,
    })
}

/// Headers the server may ask a PUT to carry: the conditional write and checksum guards.
fn allowed(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "if-none-match" || name == "content-md5" || name.starts_with("x-amz-")
}

/// The body of a ranged read: exactly the requested number of bytes arrive, or an error does.
pub(crate) struct RangeBody {
    response: reqwest::Response,
    expected: usize,
    received: usize,
}

impl RangeBody {
    /// The next chunk, or `None` once exactly the requested bytes have arrived.
    pub async fn chunk(
        &mut self,
        api: &Api,
        progress: &Progress,
    ) -> Result<Option<Bytes>, ApiError> {
        match self.response.chunk().await {
            Ok(Some(chunk)) => {
                progress.touch();
                api.counters.received(chunk.len() as u64);
                self.received += chunk.len();
                if self.received > self.expected {
                    return Err(ApiError::Malformed);
                }
                Ok(Some(chunk))
            }
            Ok(None) if self.received == self.expected => Ok(None),
            Ok(None) => Err(ApiError::Malformed),
            Err(_) => Err(ApiError::Transport),
        }
    }
}

impl Api {
    /// Starts reading bytes `start..=end` of a pack through its presigned URL. The credential
    /// is never sent.
    pub async fn open_range(&self, url: &str, start: u64, end: u64) -> Result<RangeBody, ApiError> {
        let url = only_http(url)?;
        let expected = usize::try_from(end - start + 1).map_err(|_| ApiError::Malformed)?;
        self.counters.request();
        let response = self
            .http
            .get(url)
            .header(RANGE, format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(|e| {
                trace!(error = %e, "object read failed");
                ApiError::Transport
            })?;
        if !matches!(
            response.status(),
            StatusCode::OK | StatusCode::PARTIAL_CONTENT
        ) {
            return Err(status_only(response.status()));
        }
        Ok(RangeBody {
            response,
            expected,
            received: 0,
        })
    }

    /// Writes a pack through its presigned URL with the headers the server asked for.
    pub async fn put_object(
        &self,
        url: &str,
        headers: &BTreeMap<String, String>,
        bytes: &Bytes,
    ) -> Result<(), ApiError> {
        let url = only_http(url)?;
        let mut request = self.http.put(url).body(bytes.clone());
        for (name, value) in headers.iter().filter(|(name, _)| allowed(name)) {
            let name = HeaderName::try_from(name.as_str()).map_err(|_| ApiError::Malformed)?;
            let value = HeaderValue::try_from(value.as_str()).map_err(|_| ApiError::Malformed)?;
            request = request.header(name, value);
        }
        self.counters.request();
        self.counters.sent(bytes.len() as u64);
        let response = request.send().await.map_err(|e| {
            trace!(error = %e, "object write failed");
            ApiError::Transport
        })?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(status_only(response.status()))
        }
    }
}

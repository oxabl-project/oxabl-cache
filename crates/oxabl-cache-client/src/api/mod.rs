//! Calls to the cache server: the control plane in JSON, the data plane in binary frames.

mod body;
mod cache_info;
mod dictionary;
mod error;
mod lookup;
mod object;
mod purge;
mod reports;
mod roster;
mod uploads;

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use tracing::trace;

use crate::auth::Authenticator;
use crate::progress::Progress;
use crate::stats::Counters;

pub(crate) use error::{ApiError, StatusError};
pub(crate) use object::RangeBody;
pub(crate) use uploads::{UploadRequest, UploadSlot, sha256};

/// The binary content type of the protocol.
pub(crate) const BINARY: &str = "application/vnd.oxabl-cache.v1";
const ERROR_BODY_CAP: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// A client of one cache on one server.
#[derive(Clone)]
pub(crate) struct Api {
    http: Client,
    base: String,
    auth: Arc<Authenticator>,
    counters: Arc<Counters>,
}

/// An HTTP client for the server and the object store: TLS 1.3, no redirects, no total timeout
/// so that a slow transfer that keeps moving is never cut off. `roots` are added to the
/// system roots.
pub(crate) fn http_client(
    connections: usize,
    roots: &[reqwest::Certificate],
) -> Result<Client, reqwest::Error> {
    let mut builder = Client::builder()
        .tls_version_min(reqwest::tls::Version::TLS_1_3)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .pool_max_idle_per_host(connections);
    for root in roots {
        builder = builder.add_root_certificate(root.clone());
    }
    builder.build()
}

#[derive(Deserialize)]
struct Envelope {
    error: EnvelopeError,
}

#[derive(Deserialize)]
struct EnvelopeError {
    code: Option<String>,
    reason: Option<String>,
}

fn retry_after(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse()
        .ok()
        .map(Duration::from_secs)
}

impl Api {
    pub fn new(
        http: Client,
        server: &str,
        cache: &str,
        auth: Arc<Authenticator>,
        counters: Arc<Counters>,
    ) -> Self {
        Self {
            http,
            base: format!("{}/v1/caches/{cache}", server.trim_end_matches('/')),
            auth,
            counters,
        }
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base)
    }

    async fn status_error(&self, response: Response, progress: &Progress) -> ApiError {
        let status = response.status().as_u16();
        let retry_after = retry_after(&response);
        let body = body::read_capped(response, ERROR_BODY_CAP, progress, &self.counters)
            .await
            .unwrap_or_default();
        let envelope = serde_json::from_slice::<Envelope>(&body).ok();
        ApiError::Status(StatusError {
            status,
            code: envelope.as_ref().and_then(|e| e.error.code.clone()),
            reason: envelope.and_then(|e| e.error.reason),
            retry_after,
        })
    }

    /// Sends the request `build` makes with the current credential. After a 401 it
    /// re-authenticates once, where the credential can be renewed, and sends it again.
    async fn call(
        &self,
        progress: &Progress,
        build: impl Fn(&Client) -> RequestBuilder,
    ) -> Result<Response, ApiError> {
        let mut renewed = false;
        loop {
            let (generation, token) = self.auth.bearer();
            self.counters.request();
            let response = build(&self.http)
                .bearer_auth(oxabl_cache_crypto::ExposeSecret::expose_secret(&token))
                .send()
                .await
                .map_err(|e| {
                    trace!(error = %e, "request failed");
                    ApiError::Transport
                })?;
            if response.status() == StatusCode::UNAUTHORIZED
                && !renewed
                && self.auth.refresh(generation).await
            {
                renewed = true;
                continue;
            }
            if !response.status().is_success() {
                return Err(self.status_error(response, progress).await);
            }
            return Ok(response);
        }
    }
}

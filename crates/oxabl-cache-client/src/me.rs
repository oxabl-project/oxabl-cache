//! What the signed-in principal can see on a server, without opening any cache.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::api::http_client;
use crate::auth::Authenticator;
use crate::config::{Config, check_server, roots};
use crate::error::OpenError;
use crate::state::StateDirs;
use crate::status::Quota;

const BODY_CAP: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);

/// A cache the credential holds a grant on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VisibleCache {
    /// `c_…`.
    pub id: String,
    pub display_name: String,
    /// The credential's scopes on the cache: `read`, `write:personal`, `write:trusted`,
    /// `cache:admin`. Empty for a tenant administrator without a grant on it.
    pub scopes: Vec<String>,
    pub epoch: u32,
    pub roster_version: u64,
    pub quota: Quota,
}

/// The tenant of the credential and every cache of it the credential holds a grant on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Me {
    /// `t_…`.
    pub tenant: String,
    pub tenant_admin: bool,
    pub caches: Vec<VisibleCache>,
}

/// Asks the server of `config` what its credential (a token, a CI identity or the stored login
/// session of the tenant) can see.
pub async fn me(config: &Config) -> Result<Me, OpenError> {
    check_server(&config.server)?;
    let roots = roots::load(config.ca_file.as_deref())?;
    let http = http_client(1, &roots).map_err(|_| OpenError::Unreachable)?;
    let server = config.server.as_str().trim_end_matches('/').to_owned();
    let state = StateDirs::new(config.state_dir.as_deref());
    let auth = Authenticator::establish(
        &config.credentials,
        &http,
        &server,
        config.pins.tenant,
        &state,
    )
    .await?;
    let (_, token) = auth.bearer();
    let mut response = http
        .get(format!("{server}/v1/me"))
        .bearer_auth(oxabl_cache_crypto::ExposeSecret::expose_secret(&token))
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|_| OpenError::Unreachable)?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err(OpenError::Authentication),
        status => return Err(OpenError::Server(status)),
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| OpenError::Unreachable)? {
        if body.len() + chunk.len() > BODY_CAP {
            return Err(OpenError::BadResponse);
        }
        body.extend_from_slice(&chunk);
    }
    let me: Me = serde_json::from_slice(&body).map_err(|_| OpenError::BadResponse)?;
    if me.tenant != config.pins.tenant.to_text() {
        return Err(OpenError::BadResponse);
    }
    Ok(me)
}

/// The fastest of `tries` round trips to the server's health check; no credential is sent.
pub async fn round_trip(config: &Config, tries: usize) -> Result<Duration, OpenError> {
    check_server(&config.server)?;
    let roots = roots::load(config.ca_file.as_deref())?;
    let http = http_client(1, &roots).map_err(|_| OpenError::Unreachable)?;
    let url = format!("{}/v1/health", config.server.as_str().trim_end_matches('/'));
    let mut best: Option<Duration> = None;
    for _ in 0..tries.max(1) {
        let started = Instant::now();
        let answer = http
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|_| OpenError::Unreachable)?;
        if !answer.status().is_success() {
            return Err(OpenError::Server(answer.status().as_u16()));
        }
        let took = started.elapsed();
        best = Some(best.map_or(took, |b| b.min(took)));
    }
    best.ok_or(OpenError::Unreachable)
}

/// Whether a stored login session for the tenant of `config` is present and unexpired. Says
/// nothing about whether the server still honours it.
pub fn signed_in(config: &Config) -> bool {
    StateDirs::new(config.state_dir.as_deref())
        .session(&config.pins.tenant.to_text())
        .load()
        .ok()
        .flatten()
        .is_some()
}

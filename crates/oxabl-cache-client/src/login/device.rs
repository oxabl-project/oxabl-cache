use std::time::Duration;

use oxabl_cache_crypto::{ExposeSecret, SecretString};
use reqwest::Client;
use serde::Deserialize;
use tokio::time::Instant;
use url::Url;

use super::LoginError;
use super::discovery::read_capped;
use super::endpoint::displayable;

const TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_INTERVAL: u64 = 5;
const SLOW_DOWN_STEP: u64 = 5;
const MAX_INTERVAL: u64 = 60;
const MAX_LIFETIME: u64 = 30 * 60;
const MAX_TRANSPORT_FAILURES: u32 = 3;
const GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// A pending device login: what the person is shown and what the client polls with.
pub struct Challenge {
    /// The code to type at `verification_uri`.
    pub user_code: String,
    pub verification_uri: String,
    /// The same page with the code filled in, when the issuer offers one.
    pub verification_uri_complete: Option<String>,
    pub expires_in: Duration,
    pub(super) device_code: SecretString,
    pub(super) interval: Duration,
    pub(super) token_endpoint: Url,
}

#[derive(Deserialize)]
struct Authorization {
    device_code: String,
    user_code: String,
    #[serde(alias = "verification_url")]
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct Tokens {
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct Refusal {
    error: String,
}

fn shown(url: &str) -> Result<String, LoginError> {
    let parsed =
        Url::parse(url).map_err(|_| LoginError::Protocol("the verification page is not a URL"))?;
    if !matches!(parsed.scheme(), "https" | "http") || !displayable(url) {
        return Err(LoginError::Protocol(
            "the verification page is not shown safely",
        ));
    }
    Ok(url.to_owned())
}

/// Asks the issuer for a device code.
pub(super) async fn authorize(
    http: &Client,
    device_endpoint: &Url,
    token_endpoint: &Url,
    client_id: &str,
    audience: &str,
) -> Result<Challenge, LoginError> {
    let response = http
        .post(device_endpoint.clone())
        .timeout(TIMEOUT)
        .form(&[
            ("client_id", client_id),
            ("scope", "openid"),
            ("audience", audience),
        ])
        .send()
        .await
        .map_err(|_| LoginError::Unreachable)?;
    if !response.status().is_success() {
        return Err(LoginError::Protocol("the device authorization was refused"));
    }
    let raw: Authorization = serde_json::from_slice(&read_capped(response).await?)
        .map_err(|_| LoginError::Protocol("the device authorization answer is not valid"))?;
    if raw.device_code.is_empty() || !displayable(&raw.user_code) {
        return Err(LoginError::Protocol(
            "the device authorization answer is not valid",
        ));
    }
    let interval = raw
        .interval
        .unwrap_or(DEFAULT_INTERVAL)
        .clamp(1, MAX_INTERVAL);
    Ok(Challenge {
        verification_uri: shown(&raw.verification_uri)?,
        verification_uri_complete: raw
            .verification_uri_complete
            .as_deref()
            .map(shown)
            .transpose()?,
        user_code: raw.user_code,
        expires_in: Duration::from_secs(raw.expires_in.unwrap_or(MAX_LIFETIME).min(MAX_LIFETIME)),
        device_code: SecretString::from(raw.device_code),
        interval: Duration::from_secs(interval),
        token_endpoint: token_endpoint.clone(),
    })
}

/// Polls the token endpoint until the person approves, denies, or the code expires.
pub(super) async fn await_id_token(
    http: &Client,
    challenge: &Challenge,
    client_id: &str,
) -> Result<SecretString, LoginError> {
    let deadline = Instant::now() + challenge.expires_in;
    let mut interval = challenge.interval;
    let mut failures = 0;
    loop {
        tokio::time::sleep(interval).await;
        if Instant::now() >= deadline {
            return Err(LoginError::Expired);
        }
        let response = http
            .post(challenge.token_endpoint.clone())
            .timeout(TIMEOUT)
            .form(&[
                ("grant_type", GRANT),
                ("device_code", challenge.device_code.expose_secret()),
                ("client_id", client_id),
            ])
            .send()
            .await;
        let Ok(response) = response else {
            failures += 1;
            if failures > MAX_TRANSPORT_FAILURES {
                return Err(LoginError::Unreachable);
            }
            continue;
        };
        failures = 0;
        let success = response.status().is_success();
        let body = read_capped(response).await?;
        if success {
            let tokens: Tokens = serde_json::from_slice(&body)
                .map_err(|_| LoginError::Protocol("the token answer is not valid"))?;
            return tokens
                .id_token
                .filter(|t| !t.is_empty())
                .map(SecretString::from)
                .ok_or(LoginError::Protocol("the issuer returned no ID token"));
        }
        let refusal: Refusal = serde_json::from_slice(&body)
            .map_err(|_| LoginError::Protocol("the token answer is not valid"))?;
        match refusal.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => {
                interval = (interval + Duration::from_secs(SLOW_DOWN_STEP))
                    .min(Duration::from_secs(MAX_INTERVAL));
            }
            "access_denied" => return Err(LoginError::Denied),
            "expired_token" => return Err(LoginError::Expired),
            _ => return Err(LoginError::Protocol("the issuer refused the request")),
        }
    }
}

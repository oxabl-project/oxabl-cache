use std::time::Duration;

use oxabl_cache_crypto::{ExposeSecret, SecretString};
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::error::{IdTokenError, OpenError};

const BODY_CAP: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// The GitHub Actions token endpoint of the running job.
#[derive(Clone)]
pub(crate) struct GitHubIssuer {
    url: Url,
    request_token: SecretString,
}

#[derive(Deserialize)]
struct Minted {
    value: String,
}

impl GitHubIssuer {
    /// The issuer, when the job has both variables.
    pub fn from_env() -> Result<Option<Self>, OpenError> {
        let (Some(url), Some(token)) = (
            std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
                .ok()
                .filter(|v| !v.is_empty()),
        ) else {
            return Ok(None);
        };
        let url =
            Url::parse(&url).map_err(|_| OpenError::IdToken(IdTokenError::IssuerUnavailable))?;
        Ok(Some(Self {
            url,
            request_token: SecretString::from(token),
        }))
    }

    /// Requests an ID token for `audience`.
    pub async fn mint(&self, http: &Client, audience: &str) -> Result<SecretString, OpenError> {
        let unavailable = || OpenError::IdToken(IdTokenError::IssuerUnavailable);
        let mut url = self.url.clone();
        url.query_pairs_mut().append_pair("audience", audience);
        let response = http
            .get(url)
            .timeout(TIMEOUT)
            .bearer_auth(self.request_token.expose_secret())
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(unavailable());
        }
        let bytes = response.bytes().await.map_err(|_| unavailable())?;
        if bytes.len() > BODY_CAP {
            return Err(unavailable());
        }
        let minted: Minted = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        Ok(SecretString::from(minted.value))
    }
}

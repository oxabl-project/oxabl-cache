use std::time::Duration;

use oxabl_cache_crypto::{ExposeSecret, SecretString};
use oxabl_cache_format::TenantId;
use reqwest::Client;
use serde::Deserialize;

use crate::error::OpenError;

const BODY_CAP: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// Trades an ID token for a session at `POST /v1/auth/oidc`.
#[derive(Clone)]
pub(crate) struct Exchange {
    http: Client,
    endpoint: String,
    tenant: TenantId,
}

#[derive(Deserialize)]
struct Session {
    token: String,
}

impl Exchange {
    pub fn new(http: Client, server: &str, tenant: TenantId) -> Self {
        Self {
            http,
            endpoint: format!("{}/v1/auth/oidc", server.trim_end_matches('/')),
            tenant,
        }
    }

    pub async fn session(&self, id_token: &SecretString) -> Result<SecretString, OpenError> {
        let body = serde_json::json!({
            "tenant": self.tenant.to_text(),
            "id_token": id_token.expose_secret(),
        });
        let response = self
            .http
            .post(&self.endpoint)
            .timeout(TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(|_| OpenError::Unreachable)?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(OpenError::Authentication);
        }
        if !status.is_success() {
            return Err(OpenError::Server(status.as_u16()));
        }
        let bytes = response.bytes().await.map_err(|_| OpenError::Unreachable)?;
        if bytes.len() > BODY_CAP {
            return Err(OpenError::Server(status.as_u16()));
        }
        let session: Session =
            serde_json::from_slice(&bytes).map_err(|_| OpenError::Server(status.as_u16()))?;
        Ok(SecretString::from(session.token))
    }
}

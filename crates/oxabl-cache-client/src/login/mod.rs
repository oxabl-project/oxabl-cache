//! The OAuth device authorization grant for humans: an ID token for the tenant's audience is
//! exchanged for a cache session that is stored for later commands.

mod device;
mod discovery;
mod endpoint;
mod error;

use std::time::Duration;

use oxabl_cache_format::TenantId;
use reqwest::Client;
use url::Url;

use crate::api::http_client;
use crate::auth::Exchange;
use crate::auth::id_token::{audience, check_audience};
use crate::config::{Config, check_server, roots};
use crate::state::StateDirs;

pub use device::Challenge;
pub use error::LoginError;

/// A person's login against one issuer for one tenant.
pub struct DeviceLogin {
    http: Client,
    issuer: Url,
    client_id: String,
    server: String,
    tenant: TenantId,
    state: StateDirs,
}

/// A session the cache server opened and the client stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginSession {
    /// Unix seconds.
    pub expires_at: u64,
}

impl DeviceLogin {
    /// A login for the tenant and server of `config`. The ID token the issuer returns must
    /// carry the audience `oxabl-cache:<tenant>`, and the issuer must be registered for the
    /// tenant on the server.
    pub fn new(config: &Config, issuer: &str, client_id: &str) -> Result<Self, LoginError> {
        let issuer = Url::parse(issuer).map_err(|_| LoginError::Issuer("not a URL"))?;
        endpoint::check_endpoint(&issuer)
            .map_err(|_| LoginError::Issuer("the issuer URL must be https"))?;
        check_server(&config.server)?;
        if !endpoint::displayable(client_id) {
            return Err(LoginError::Config(crate::config::ConfigError::Invalid(
                "client id",
            )));
        }
        let roots = roots::load(config.ca_file.as_deref())?;
        Ok(Self {
            http: http_client(1, &roots).map_err(|_| LoginError::Unreachable)?,
            issuer,
            client_id: client_id.to_owned(),
            server: config.server.as_str().trim_end_matches('/').to_owned(),
            tenant: config.pins.tenant,
            state: StateDirs::new(config.state_dir.as_deref()),
        })
    }

    /// Asks the issuer for a device code to show the person.
    pub async fn begin(&self) -> Result<Challenge, LoginError> {
        let endpoints = discovery::discover(&self.http, &self.issuer).await?;
        device::authorize(
            &self.http,
            &endpoints.device,
            &endpoints.token,
            &self.client_id,
            &audience(&self.tenant),
        )
        .await
    }

    /// Waits for the approval, exchanges the ID token and stores the session.
    pub async fn finish(&self, challenge: &Challenge) -> Result<LoginSession, LoginError> {
        let id_token = device::await_id_token(&self.http, challenge, &self.client_id).await?;
        check_audience(&id_token, &audience(&self.tenant)).map_err(LoginError::IdToken)?;
        let opened = Exchange::new(self.http.clone(), &self.server, self.tenant)
            .open_session(&id_token)
            .await
            .map_err(LoginError::Exchange)?;
        self.state
            .session(&self.tenant.to_text())
            .save(&opened.token, opened.expires_at)
            .map_err(|e| LoginError::State(e.kind()))?;
        Ok(LoginSession {
            expires_at: opened.expires_at,
        })
    }
}

/// What `logout` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Logout {
    /// A stored session existed and its file is gone.
    pub removed: bool,
    /// The server confirmed that the session no longer opens anything. False when there was
    /// none to revoke, or the server could not be asked.
    pub revoked: bool,
}

/// Revokes the stored session at the server and deletes its file. The file goes even when
/// the server cannot be reached.
pub async fn logout(config: &Config) -> Result<Logout, LoginError> {
    check_server(&config.server)?;
    let session =
        StateDirs::new(config.state_dir.as_deref()).session(&config.pins.tenant.to_text());
    let Some(token) = session
        .load_any()
        .map_err(|e| LoginError::State(e.kind()))?
    else {
        return Ok(Logout {
            removed: false,
            revoked: false,
        });
    };
    let roots = roots::load(config.ca_file.as_deref())?;
    let http = http_client(1, &roots).map_err(|_| LoginError::Unreachable)?;
    let answer = http
        .delete(format!(
            "{}/v1/auth/session",
            config.server.as_str().trim_end_matches('/')
        ))
        .bearer_auth(oxabl_cache_crypto::ExposeSecret::expose_secret(&token))
        .timeout(Duration::from_secs(30))
        .send()
        .await;
    let revoked = answer.is_ok_and(|r| r.status().is_success() || r.status().as_u16() == 401);
    let removed = session.remove().map_err(|e| LoginError::State(e.kind()))?;
    Ok(Logout { removed, revoked })
}

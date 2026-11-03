//! Credentials: a bearer for the server, obtained from a token, a CI system or a stored session.

mod exchange;
mod github;
mod id_token;

use std::sync::Mutex;

use oxabl_cache_crypto::SecretString;
use oxabl_cache_format::TenantId;
use reqwest::Client;
use tracing::debug;

use crate::config::Credentials;
use crate::error::OpenError;
use crate::state::StateDirs;

use exchange::Exchange;
use github::GitHubIssuer;

/// How a session can be obtained again once it expires.
enum Renewal {
    None,
    GitHub(GitHubIssuer),
    IdToken(SecretString),
}

/// Holds the bearer for the server and renews it once after a 401 where it can.
pub(crate) struct Authenticator {
    current: Mutex<(u64, SecretString)>,
    renewal: Renewal,
    exchange: Exchange,
    tenant: TenantId,
    http: Client,
    gate: tokio::sync::Mutex<()>,
}

fn environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl Authenticator {
    /// Resolves `credentials`, exchanging an ID token for a session where one is the source.
    /// An ID token whose audience is not `oxabl-cache:<tenant>` never leaves the process.
    pub async fn establish(
        credentials: &Credentials,
        http: &Client,
        server: &str,
        tenant: TenantId,
        state: &StateDirs,
    ) -> Result<Self, OpenError> {
        let exchange = Exchange::new(http.clone(), server, tenant);
        let audience = id_token::audience(&tenant);
        let (token, renewal) = match credentials {
            Credentials::Token(token) => (token.clone(), Renewal::None),
            Credentials::OidcIdToken(id_token) => {
                id_token::check_audience(id_token, &audience).map_err(OpenError::IdToken)?;
                (
                    exchange.session(id_token).await?,
                    Renewal::IdToken(id_token.clone()),
                )
            }
            Credentials::Auto => {
                if let Some(token) = environment("OXABL_CACHE_TOKEN") {
                    (SecretString::from(token), Renewal::None)
                } else if let Some(issuer) = GitHubIssuer::from_env()? {
                    debug!("requesting a GitHub Actions ID token");
                    let id_token = issuer.mint(http, &audience).await?;
                    id_token::check_audience(&id_token, &audience).map_err(OpenError::IdToken)?;
                    (exchange.session(&id_token).await?, Renewal::GitHub(issuer))
                } else if let Some(id_token) = environment("OXABL_CACHE_ID_TOKEN") {
                    let id_token = SecretString::from(id_token);
                    id_token::check_audience(&id_token, &audience).map_err(OpenError::IdToken)?;
                    (
                        exchange.session(&id_token).await?,
                        Renewal::IdToken(id_token),
                    )
                } else {
                    let stored = state
                        .session(&tenant.to_text())
                        .load()
                        .map_err(|e| OpenError::State(e.kind()))?;
                    (stored.ok_or(OpenError::NoCredentials)?, Renewal::None)
                }
            }
        };
        Ok(Self {
            current: Mutex::new((0, token)),
            renewal,
            exchange,
            tenant,
            http: http.clone(),
            gate: tokio::sync::Mutex::new(()),
        })
    }

    /// The current bearer and its generation.
    pub fn bearer(&self) -> (u64, SecretString) {
        let guard = self.current.lock().expect("credential lock");
        (guard.0, guard.1.clone())
    }

    /// Renews the session if `seen` is still the current generation. False when the credential
    /// cannot be renewed or the renewal failed.
    pub async fn refresh(&self, seen: u64) -> bool {
        if matches!(self.renewal, Renewal::None) {
            return false;
        }
        let _gate = self.gate.lock().await;
        if self.current.lock().expect("credential lock").0 != seen {
            return true;
        }
        let audience = id_token::audience(&self.tenant);
        let id_token = match &self.renewal {
            Renewal::None => return false,
            Renewal::GitHub(issuer) => match issuer.mint(&self.http, &audience).await {
                Ok(token) => token,
                Err(_) => return false,
            },
            Renewal::IdToken(token) => token.clone(),
        };
        if id_token::check_audience(&id_token, &audience).is_err() {
            return false;
        }
        match self.exchange.session(&id_token).await {
            Ok(session) => {
                *self.current.lock().expect("credential lock") = (seen + 1, session);
                true
            }
            Err(_) => false,
        }
    }
}

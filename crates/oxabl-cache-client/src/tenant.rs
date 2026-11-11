//! The tenant administrator's API tokens: create, list and revoke.

use std::fmt;
use std::time::Duration;

use oxabl_cache_crypto::{ExposeSecret, SecretString};
use reqwest::{Client, Method};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::http_client;
use crate::config::{Config, ConfigError, check_server, roots};
use crate::error::OpenError;
use crate::me::Me;

const BODY_CAP: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// Why a tenant call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TenantError {
    Open(OpenError),
    /// The server does not know the token: it is wrong, expired, revoked or of another server.
    Authentication,
    /// The token is valid but not a `tenant:admin` token.
    NotTenantAdmin,
    /// The token belongs to another tenant than the repository's.
    WrongTenant,
    /// The input is malformed.
    Invalid(&'static str),
    /// The server answered an error.
    Refused {
        status: u16,
        code: Option<String>,
        message: Option<String>,
    },
    Unreachable,
    /// The server's answer is not what the protocol defines.
    Malformed,
}

impl fmt::Display for TenantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(e) => e.fmt(f),
            Self::Authentication => {
                f.write_str("the server does not accept the token: it is wrong, expired or revoked")
            }
            Self::NotTenantAdmin => f.write_str("the token does not carry tenant:admin"),
            Self::WrongTenant => {
                f.write_str("the token belongs to another tenant than the cache's")
            }
            Self::Invalid(what) => write!(f, "invalid input: {what}"),
            Self::Refused {
                status,
                code,
                message,
            } => {
                write!(f, "the server refused the request: {status}")?;
                if let Some(code) = code {
                    write!(f, " {code}")?;
                }
                if let Some(message) = message {
                    write!(f, " ({message})")?;
                }
                Ok(())
            }
            Self::Unreachable => f.write_str("the cache server is unreachable"),
            Self::Malformed => f.write_str("the cache server sent a malformed answer"),
        }
    }
}

impl std::error::Error for TenantError {}

impl From<ConfigError> for TenantError {
    fn from(e: ConfigError) -> Self {
        Self::Open(e.into())
    }
}

/// What a new token carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewToken {
    /// A label for the token list.
    pub name: String,
    /// Protocol scope names: `read`, `write:personal`, `write:trusted`, `cache:admin`,
    /// `tenant:admin`.
    pub scopes: Vec<String>,
    /// The caches the cache scopes apply to, `c_…`.
    pub caches: Vec<String>,
    /// The lifetime; the server's default and maximum apply when `None` or too long.
    pub expires_in_days: Option<u32>,
}

/// A token the server created. The secret exists only here: the server keeps a hash.
#[derive(Debug, Clone)]
pub struct CreatedToken {
    pub id: String,
    pub token: SecretString,
    pub principal: String,
    /// Unix seconds.
    pub expires_at: i64,
}

/// One scope set on one cache, or on the whole tenant when `cache` is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TokenGrant {
    pub cache: Option<String>,
    pub scopes: Vec<String>,
}

/// A token of the tenant, without its secret.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TokenInfo {
    pub id: String,
    pub name: String,
    pub principal: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
    pub grants: Vec<TokenGrant>,
}

#[derive(Deserialize)]
struct Created {
    id: String,
    token: String,
    principal: String,
    expires_at: i64,
}

#[derive(Deserialize)]
struct Listing {
    tokens: Vec<TokenInfo>,
}

#[derive(Deserialize)]
struct Envelope {
    error: EnvelopeError,
}

#[derive(Deserialize)]
struct EnvelopeError {
    code: Option<String>,
    message: Option<String>,
}

/// The tenant API as a `tenant:admin` token uses it.
pub struct TenantAdmin {
    http: Client,
    server: String,
    token: SecretString,
    me: Me,
}

impl TenantAdmin {
    /// Checks with the server that `token` is a `tenant:admin` token of the tenant that
    /// `config` pins. Nothing else of `config` is used.
    pub async fn open(config: &Config, token: SecretString) -> Result<Self, TenantError> {
        check_server(&config.server)?;
        let roots = roots::load(config.ca_file.as_deref())?;
        let http = http_client(1, &roots).map_err(|_| TenantError::Unreachable)?;
        let server = config.server.as_str().trim_end_matches('/').to_owned();
        let mut admin = Self {
            http,
            server,
            token,
            me: Me {
                tenant: String::new(),
                tenant_admin: false,
                caches: Vec::new(),
            },
        };
        let body = admin.send(Method::GET, "/v1/me", None).await?;
        admin.me = serde_json::from_slice(&body).map_err(|_| TenantError::Malformed)?;
        if admin.me.tenant != config.tenant() {
            return Err(TenantError::WrongTenant);
        }
        if !admin.me.tenant_admin {
            return Err(TenantError::NotTenantAdmin);
        }
        Ok(admin)
    }

    /// Whether the tenant has the cache `cache`, `c_…`.
    pub fn has_cache(&self, cache: &str) -> bool {
        self.me.caches.iter().any(|c| c.id == cache)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Vec<u8>, TenantError> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.server))
            .bearer_auth(self.token.expose_secret())
            .timeout(TIMEOUT);
        if let Some(body) = body {
            request = request.json(body);
        }
        let mut response = request.send().await.map_err(|_| TenantError::Unreachable)?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| TenantError::Unreachable)?
        {
            if bytes.len() + chunk.len() > BODY_CAP {
                return Err(TenantError::Malformed);
            }
            bytes.extend_from_slice(&chunk);
        }
        match status {
            200..=299 => Ok(bytes),
            401 => Err(TenantError::Authentication),
            _ => {
                let envelope = serde_json::from_slice::<Envelope>(&bytes).ok();
                Err(TenantError::Refused {
                    status,
                    code: envelope.as_ref().and_then(|e| e.error.code.clone()),
                    message: envelope.and_then(|e| e.error.message),
                })
            }
        }
    }

    /// Creates a token. The server needs a name, at least one scope and, for cache scopes, at
    /// least one existing cache.
    pub async fn create_token(&self, spec: &NewToken) -> Result<CreatedToken, TenantError> {
        let mut body = json!({
            "name": spec.name,
            "scopes": spec.scopes,
            "caches": spec.caches,
        });
        if let Some(days) = spec.expires_in_days {
            body["expires_in_days"] = days.into();
        }
        let answer = self
            .send(Method::POST, "/v1/tenant/tokens", Some(&body))
            .await?;
        let created: Created =
            serde_json::from_slice(&answer).map_err(|_| TenantError::Malformed)?;
        Ok(CreatedToken {
            id: created.id,
            token: SecretString::from(created.token),
            principal: created.principal,
            expires_at: created.expires_at,
        })
    }

    /// Every token of the tenant, revoked ones included.
    pub async fn list_tokens(&self) -> Result<Vec<TokenInfo>, TenantError> {
        let answer = self.send(Method::GET, "/v1/tenant/tokens", None).await?;
        let listing: Listing =
            serde_json::from_slice(&answer).map_err(|_| TenantError::Malformed)?;
        Ok(listing.tokens)
    }

    /// Revokes the token `id` at once.
    pub async fn revoke_token(&self, id: &str) -> Result<(), TenantError> {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(TenantError::Invalid(
                "a token id is letters, digits, _ and -",
            ));
        }
        self.send(Method::DELETE, &format!("/v1/tenant/tokens/{id}"), None)
            .await
            .map(drop)
    }
}

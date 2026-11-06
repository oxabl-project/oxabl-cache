//! A one-line invitation to a cache: what `.oxabl/cache.toml` pins, plus the issuer a person
//! signs in with, if the organisation has one. It holds no secret, so it travels by chat or
//! mail; a checksum catches a damaged copy.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use oxabl_cache_format::{CacheId, PublicKey, TenantId};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::{
    Config, ConfigError, DEFAULT_CONNECTIONS, Pins, check_server, owner_key_text, parse_owner_key,
};
use crate::mode::Mode;

const PREFIX: &str = "oxc-invite:";
const VERSION: u32 = 1;
const MAX_CHARS: usize = 8 * 1024;
const MAX_NAME: usize = 64;
const CHECK_LEN: usize = 4;

fn check_bytes(json: &[u8]) -> [u8; CHECK_LEN] {
    let mut check = [0u8; CHECK_LEN];
    check.copy_from_slice(&blake3::hash(json).as_bytes()[..CHECK_LEN]);
    check
}

/// A cache and how to join it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    server: Url,
    tenant: TenantId,
    cache: CacheId,
    owner_keys: Vec<PublicKey>,
    min_roster_version: u64,
    sign_in: Option<SignIn>,
    name: String,
}

/// Where the invited sign in: an OIDC issuer and its public client.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SignIn {
    issuer: Url,
    client_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    v: u32,
    name: String,
    server: String,
    tenant: String,
    cache: String,
    owner_keys: Vec<String>,
    min_roster_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
}

fn displayable(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.chars().count() <= limit && !text.chars().any(char::is_control)
}

impl Invite {
    /// An invitation to the cache `config` pins, named `name`. With `sign_in`, an issuer and
    /// its public client id, members sign in there; without, they bring an access token.
    pub fn new(
        config: &Config,
        name: &str,
        sign_in: Option<(&str, &str)>,
    ) -> Result<Invite, ConfigError> {
        let invite = Invite {
            server: config.server.clone(),
            tenant: config.pins.tenant,
            cache: config.pins.cache,
            owner_keys: config.pins.owner_keys.clone(),
            min_roster_version: config.pins.min_roster_version,
            sign_in: sign_in
                .map(|(issuer, client_id)| SignIn::new(issuer, client_id.to_owned()))
                .transpose()?,
            name: name.trim().to_owned(),
        };
        invite.check()?;
        Ok(invite)
    }

    fn check(&self) -> Result<(), ConfigError> {
        check_server(&self.server)?;
        if let Some(sign_in) = &self.sign_in {
            if sign_in.issuer.scheme() != "https" && check_server(&sign_in.issuer).is_err() {
                return Err(ConfigError::Invalid("the issuer must be https"));
            }
            if !displayable(&sign_in.client_id, 256) {
                return Err(ConfigError::Invalid("client id"));
            }
        }
        if !displayable(&self.name, MAX_NAME) {
            return Err(ConfigError::Invalid("name"));
        }
        if self.owner_keys.is_empty() {
            return Err(ConfigError::Invalid("owner_keys is empty"));
        }
        Ok(())
    }

    /// Reads an invitation; surrounding whitespace is ignored.
    pub fn parse(text: &str) -> Result<Invite, ConfigError> {
        let text = text.trim();
        if text.len() > MAX_CHARS {
            return Err(ConfigError::Invalid("the invite is too long"));
        }
        let encoded = text
            .strip_prefix(PREFIX)
            .ok_or(ConfigError::Invalid("an invite starts with oxc-invite:"))?;
        let damaged = ConfigError::Invalid("the invite is damaged");
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| damaged.clone())?;
        let split = bytes.len().checked_sub(CHECK_LEN).ok_or(damaged.clone())?;
        let (json, check) = bytes.split_at(split);
        if check != check_bytes(json) {
            return Err(damaged);
        }
        let wire: Wire = serde_json::from_slice(json).map_err(|_| damaged)?;
        if wire.v != VERSION {
            return Err(ConfigError::Invalid(
                "the invite needs a newer version of this app",
            ));
        }
        let invite = Invite {
            server: Url::parse(&wire.server).map_err(|_| ConfigError::Invalid("server"))?,
            tenant: TenantId::from_text(&wire.tenant)
                .map_err(|_| ConfigError::Invalid("tenant"))?,
            cache: CacheId::from_text(&wire.cache).map_err(|_| ConfigError::Invalid("cache"))?,
            owner_keys: wire
                .owner_keys
                .iter()
                .map(|key| parse_owner_key(key))
                .collect::<Result<_, _>>()?,
            min_roster_version: wire.min_roster_version,
            sign_in: match (wire.issuer, wire.client_id) {
                (Some(issuer), Some(client_id)) => Some(SignIn::new(&issuer, client_id)?),
                (None, None) => None,
                _ => return Err(ConfigError::Invalid("the invite is damaged")),
            },
            name: wire.name,
        };
        invite.check()?;
        Ok(invite)
    }

    /// The one-line form `parse` reads.
    pub fn to_text(&self) -> String {
        let wire = Wire {
            v: VERSION,
            name: self.name.clone(),
            server: self.server.to_string(),
            tenant: self.tenant.to_text(),
            cache: self.cache.to_text(),
            owner_keys: self.owner_keys.iter().map(owner_key_text).collect(),
            min_roster_version: self.min_roster_version,
            issuer: self.sign_in.as_ref().map(|s| s.issuer.to_string()),
            client_id: self.sign_in.as_ref().map(|s| s.client_id.clone()),
        };
        let mut bytes = serde_json::to_vec(&wire).expect("an invite serialises");
        let check = check_bytes(&bytes);
        bytes.extend_from_slice(&check);
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
    }

    /// A developer configuration of the invited cache; trust comes from the invite.
    pub fn config(&self) -> Config {
        Config {
            pins: Pins {
                tenant: self.tenant,
                cache: self.cache,
                owner_keys: self.owner_keys.clone(),
                min_roster_version: self.min_roster_version,
            },
            server: self.server.clone(),
            mode: Mode::Developer,
            credentials: crate::Credentials::Auto,
            identity: crate::IdentitySource::Auto,
            passphrase: None,
            state_dir: None,
            ca_file: None,
            connections: DEFAULT_CONNECTIONS,
            zstd_level: oxabl_cache_crypto::DEFAULT_ZSTD_LEVEL,
            max_plain_bytes: oxabl_cache_format::limits::DEFAULT_PLAINTEXT_BYTES,
        }
    }

    /// The `.oxabl/cache.toml` that pins the same cache.
    pub fn cache_toml(&self) -> String {
        let keys: Vec<String> = self
            .owner_keys
            .iter()
            .map(|key| format!("\"{}\"", owner_key_text(key)))
            .collect();
        format!(
            "server = \"{}\"\ntenant = \"{}\"\ncache = \"{}\"\nowner_keys = [{}]\nmin_roster_version = {}\n",
            self.server.as_str().trim_end_matches('/'),
            self.tenant.to_text(),
            self.cache.to_text(),
            keys.join(", "),
            self.min_roster_version
        )
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn server(&self) -> &Url {
        &self.server
    }

    pub fn tenant(&self) -> String {
        self.tenant.to_text()
    }

    pub fn cache(&self) -> String {
        self.cache.to_text()
    }

    /// The issuer and public client id members sign in with; `None` when they bring a token.
    pub fn sign_in(&self) -> Option<(&str, &str)> {
        self.sign_in
            .as_ref()
            .map(|s| (s.issuer.as_str(), s.client_id.as_str()))
    }
}

impl SignIn {
    fn new(issuer: &str, client_id: String) -> Result<SignIn, ConfigError> {
        Ok(SignIn {
            issuer: Url::parse(issuer).map_err(|_| ConfigError::Invalid("issuer"))?,
            client_id,
        })
    }
}

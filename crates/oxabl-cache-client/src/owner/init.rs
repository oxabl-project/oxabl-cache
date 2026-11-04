use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use oxabl_cache_crypto::{Identity, MemberEntry, RosterDraft};
use oxabl_cache_format::{CacheId, Role, TenantId};
use url::Url;

use super::error::OwnerError;
use super::members::check_label;
use crate::api::{Api, http_client};
use crate::auth::Authenticator;
use crate::config::{ConfigError, Credentials, check_server, owner_key_text, roots};
use crate::error::OpenError;
use crate::progress::Progress;
use crate::state::StateDirs;
use crate::stats::Counters;

/// What `init` needs: where the cache is, who creates its roster and how it authenticates.
pub struct InitRequest {
    server: Url,
    tenant: String,
    cache: String,
    label: String,
    identity: Identity,
    credentials: Credentials,
    state_dir: Option<PathBuf>,
    ca_file: Option<PathBuf>,
}

impl InitRequest {
    /// The owner `identity` of the cache `cache` of `tenant` on `server`, listed as `label`.
    pub fn new(server: Url, tenant: &str, cache: &str, label: &str, identity: Identity) -> Self {
        Self {
            server,
            tenant: tenant.to_owned(),
            cache: cache.to_owned(),
            label: label.to_owned(),
            identity,
            credentials: Credentials::Auto,
            state_dir: None,
            ca_file: None,
        }
    }

    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = credentials;
        self
    }

    pub fn with_state_dir(mut self, dir: PathBuf) -> Self {
        self.state_dir = Some(dir);
        self
    }

    pub fn with_ca_file(mut self, path: PathBuf) -> Self {
        self.ca_file = Some(path);
        self
    }
}

/// The first roster, published, and what the repository and the operator record about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// The contents of `.oxabl/cache.toml`.
    pub cache_toml: String,
    /// The owner's signing key in the spelling of `owner_keys`.
    pub owner_key: String,
    pub roster_version: u64,
    pub epoch: u32,
}

fn invalid(what: &'static str) -> OwnerError {
    OwnerError::Open(OpenError::Config(ConfigError::Invalid(what)))
}

/// Creates the first roster of a cache: version 1, epoch 1, one owner, signed by that owner.
/// The cache must exist already with the owner's key registered, and the credential needs
/// `cache:admin`. The server refuses a second first roster.
pub async fn init(request: InitRequest) -> Result<InitReport, OwnerError> {
    check_server(&request.server).map_err(|e| OwnerError::Open(e.into()))?;
    let tenant = TenantId::from_text(&request.tenant).map_err(|_| invalid("tenant"))?;
    let cache = CacheId::from_text(&request.cache).map_err(|_| invalid("cache"))?;
    check_label(&request.label)?;
    let roots = roots::load(request.ca_file.as_deref()).map_err(|e| OwnerError::Open(e.into()))?;
    let http = http_client(32, &roots).map_err(|_| OwnerError::Unreachable)?;
    let state = StateDirs::new(request.state_dir.as_deref());
    let server = request.server.as_str().trim_end_matches('/').to_owned();
    let auth = Authenticator::establish(&request.credentials, &http, &server, tenant, &state)
        .await
        .map_err(OwnerError::Open)?;
    let api = Api::new(
        http,
        &server,
        &cache.to_text(),
        Arc::new(auth),
        Arc::new(Counters::default()),
    );
    if api
        .roster(&Progress::new())
        .await
        .map_err(OwnerError::from_api)?
        .is_some()
    {
        return Err(OwnerError::AlreadyInitialised);
    }
    let descriptor = request.identity.descriptor()?;
    let owner = MemberEntry::from_descriptor(&request.label, Role::Owner, &descriptor)?;
    let draft = RosterDraft::genesis(cache, vec![owner])?;
    let bytes = draft.sign(&[&request.identity])?;
    api.put_roster(0, &Bytes::from(bytes))
        .await
        .map_err(OwnerError::from_api)?;
    if let Err(error) = state.high_water(&cache.to_text()).advance(draft.version()) {
        tracing::warn!(%error, "the roster high-water mark was not saved");
    }
    let owner_key = owner_key_text(&request.identity.signing_public());
    Ok(InitReport {
        cache_toml: format!(
            "server = \"{server}\"\ntenant = \"{}\"\ncache = \"{}\"\nowner_keys = [\"{owner_key}\"]\nmin_roster_version = {}\n",
            tenant.to_text(),
            cache.to_text(),
            draft.version(),
        ),
        owner_key,
        roster_version: draft.version(),
        epoch: draft.epoch(),
    })
}

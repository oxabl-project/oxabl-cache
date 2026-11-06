//! Client configuration: the committed trust root plus caller and environment settings.

mod credentials;
mod environment;
mod error;
mod owner_key;
mod repo_file;
pub(crate) mod roots;
mod server;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oxabl_cache_crypto::SecretString;
use oxabl_cache_format::limits::DEFAULT_PLAINTEXT_BYTES;
use oxabl_cache_format::{CacheId, PublicKey, TenantId};
use url::Url;

use crate::mode::Mode;

pub use credentials::{Credentials, IdentitySource};
pub use error::ConfigError;
pub(crate) use owner_key::{owner_key_text, parse_owner_key};
pub(crate) use server::check_server;

use environment::Environment;
use repo_file::RepoFile;

/// Default number of concurrent connections.
pub const DEFAULT_CONNECTIONS: usize = 32;
/// Largest number of concurrent connections.
pub const MAX_CONNECTIONS: usize = 128;

/// Supplies the passphrase of a protected identity file.
pub(crate) type PassphraseProvider = Arc<dyn Fn() -> Option<SecretString> + Send + Sync>;

/// The cache identity and root of trust, which only `.oxabl/cache.toml` can set.
#[derive(Debug, Clone)]
pub(crate) struct Pins {
    pub tenant: TenantId,
    pub cache: CacheId,
    pub owner_keys: Vec<PublicKey>,
    pub min_roster_version: u64,
}

/// Configuration; trust fields come only from `.oxabl/cache.toml`.
#[derive(Clone)]
pub struct Config {
    pub(crate) pins: Pins,
    pub(crate) server: Url,
    pub(crate) mode: Mode,
    pub(crate) credentials: Credentials,
    pub(crate) identity: IdentitySource,
    pub(crate) passphrase: Option<PassphraseProvider>,
    pub(crate) state_dir: Option<PathBuf>,
    pub(crate) ca_file: Option<PathBuf>,
    pub(crate) connections: usize,
    pub(crate) zstd_level: i32,
    pub(crate) max_plain_bytes: u64,
}

impl Config {
    /// Reads `<root>/.oxabl/cache.toml` and the environment.
    ///
    /// The file holds exactly `server`, `tenant`, `cache`, `owner_keys` and
    /// `min_roster_version`. Setting `OXABL_CACHE_TENANT`, `OXABL_CACHE_CACHE`,
    /// `OXABL_CACHE_OWNER_KEYS` or `OXABL_CACHE_MIN_ROSTER_VERSION` is a
    /// [`ConfigError::TrustOverride`].
    pub fn from_repo(root: &Path) -> Result<Config, ConfigError> {
        let environment = Environment::read()?;
        let file = RepoFile::read(root)?;
        let server = match environment.server {
            Some(text) => Url::parse(&text).map_err(|_| ConfigError::Invalid("server"))?,
            None => file.server,
        };
        Ok(Config {
            pins: Pins {
                tenant: file.tenant,
                cache: file.cache,
                owner_keys: file.owner_keys,
                min_roster_version: file.min_roster_version,
            },
            server,
            mode: environment.mode,
            credentials: Credentials::Auto,
            identity: IdentitySource::Auto,
            passphrase: None,
            state_dir: None,
            ca_file: None,
            connections: DEFAULT_CONNECTIONS,
            zstd_level: oxabl_cache_crypto::DEFAULT_ZSTD_LEVEL,
            max_plain_bytes: DEFAULT_PLAINTEXT_BYTES,
        })
    }

    /// The server this configuration talks to.
    pub fn server(&self) -> &Url {
        &self.server
    }

    /// The tenant id, `t_…`.
    pub fn tenant(&self) -> String {
        self.pins.tenant.to_text()
    }

    /// The cache id, `c_…`.
    pub fn cache(&self) -> String {
        self.pins.cache.to_text()
    }

    /// The last `limit` failures of runs of this cache on this machine, newest first. Reads the
    /// log under the state directory; a missing or unreadable one is empty.
    pub fn recent_errors(&self, limit: usize) -> Vec<crate::LoggedError> {
        crate::state::StateDirs::new(self.state_dir.as_deref())
            .error_log(&self.pins.tenant.to_text(), &self.pins.cache.to_text())
            .recent(limit)
    }

    /// The fetches and stores this machine ran against this cache, oldest first, read from the
    /// journal under the state directory; a missing or unreadable one is empty.
    pub fn local_activity(&self) -> Vec<crate::Activity> {
        crate::state::StateDirs::new(self.state_dir.as_deref())
            .journal(&self.pins.tenant.to_text(), &self.pins.cache.to_text())
            .entries()
            .into_iter()
            .map(|entry| entry.activity)
            .collect()
    }

    pub fn with_mode(mut self, mode: Mode) -> Config {
        self.mode = mode;
        self
    }

    pub fn with_credentials(mut self, credentials: Credentials) -> Config {
        self.credentials = credentials;
        self
    }

    pub fn with_identity(mut self, identity: IdentitySource) -> Config {
        self.identity = identity;
        self
    }

    pub fn with_server(mut self, url: Url) -> Config {
        self.server = url;
        self
    }

    /// Concurrent connections, between 1 and 128.
    pub fn with_concurrency(mut self, connections: usize) -> Config {
        self.connections = connections.clamp(1, MAX_CONNECTIONS);
        self
    }

    pub fn with_zstd_level(mut self, level: i32) -> Config {
        self.zstd_level = level;
        self
    }

    /// Plaintext cap per artifact set, at most 1 GiB.
    pub fn with_max_plain_bytes(mut self, bytes: u64) -> Config {
        self.max_plain_bytes = bytes;
        self
    }

    /// Where the roster high-water mark lives instead of `$XDG_STATE_HOME/oxabl-cache`.
    pub fn with_state_dir(mut self, dir: PathBuf) -> Config {
        self.state_dir = Some(dir);
        self
    }

    /// A PEM bundle of certificates to trust in addition to the system roots, for a server or an
    /// identity provider behind a private CA.
    pub fn with_ca_file(mut self, path: PathBuf) -> Config {
        self.ca_file = Some(path);
        self
    }

    /// Asks `provider` for the passphrase of a protected identity file; without a provider the
    /// client reads `OXABL_CACHE_IDENTITY_PASSPHRASE`.
    pub fn with_passphrase(
        mut self,
        provider: impl Fn() -> Option<SecretString> + Send + Sync + 'static,
    ) -> Config {
        self.passphrase = Some(Arc::new(provider));
        self
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("server", &self.server.as_str())
            .field("tenant", &self.pins.tenant.to_text())
            .field("cache", &self.pins.cache.to_text())
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

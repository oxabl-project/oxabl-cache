use std::path::PathBuf;

use oxabl_cache_crypto::{Error, Identity, SecretString, identity_env, identity_file};

use crate::config::{IdentitySource, PassphraseProvider};
use crate::error::OpenError;

fn describe(error: Error) -> OpenError {
    match error {
        Error::Identity(what) => OpenError::Identity(what),
        Error::Limit(what) => OpenError::Identity(what),
        _ => OpenError::Identity("cannot read the identity"),
    }
}

fn default_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("OXABL_CACHE_IDENTITY_FILE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let config = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").filter(|v| !v.is_empty())?).join(".config"),
    };
    Some(config.join("oxabl-cache").join("identity"))
}

fn passphrase(provider: Option<&PassphraseProvider>) -> Result<SecretString, OpenError> {
    let given = match provider {
        Some(provider) => provider(),
        None => std::env::var("OXABL_CACHE_IDENTITY_PASSPHRASE")
            .ok()
            .filter(|v| !v.is_empty())
            .map(SecretString::from),
    };
    given.ok_or(OpenError::Identity("no passphrase for the identity file"))
}

fn from_file(
    path: &std::path::Path,
    provider: Option<&PassphraseProvider>,
) -> Result<Identity, OpenError> {
    if !path.exists() {
        return Err(OpenError::Identity("no identity file"));
    }
    identity_file::load(path, &passphrase(provider)?).map_err(describe)
}

/// Resolves the member identity. Unlocking a protected file takes about a second, so this
/// runs off the async threads.
pub(crate) fn resolve(
    source: &IdentitySource,
    provider: Option<&PassphraseProvider>,
) -> Result<Identity, OpenError> {
    match source {
        IdentitySource::Value(value) => {
            Identity::parse(oxabl_cache_crypto::ExposeSecret::expose_secret(value))
                .map_err(describe)
        }
        IdentitySource::File(path) => from_file(path, provider),
        IdentitySource::Auto => {
            if std::env::var_os(identity_env::IDENTITY_VARIABLE).is_some_and(|v| !v.is_empty()) {
                return identity_env::from_env().map_err(describe);
            }
            let path = default_file().ok_or(OpenError::Identity("no identity"))?;
            from_file(&path, provider)
        }
    }
}

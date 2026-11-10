//! The identity passphrase and access tokens in the operating system's keyring, so a person
//! never types them: the Secret Service on Linux. Without the `os-keyring` feature, or on
//! another system, nothing is stored and nothing is found.

use std::path::Path;

use oxabl_cache_crypto::SecretString;
use url::Url;

const SERVICE: &str = "oxabl-cache";

/// Why the keyring could not store or remove a passphrase.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyringError {
    /// No keyring is running, or this build has none; the reason says which.
    Unavailable(String),
    /// The keyring refused or failed.
    Refused(String),
}

impl std::fmt::Display for KeyringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "no keyring is available: {why}"),
            Self::Refused(why) => write!(f, "the keyring refused: {why}"),
        }
    }
}

impl std::error::Error for KeyringError {}

fn account(identity_file: &Path) -> String {
    let absolute = std::path::absolute(identity_file).unwrap_or_else(|_| identity_file.into());
    format!("identity {}", absolute.display())
}

#[cfg(all(feature = "os-keyring", target_os = "linux"))]
mod store {
    use keyring_core::api::CredentialStoreApi;
    use keyring_core::{Entry, Error};

    use super::{KeyringError, SERVICE};

    pub(super) fn entry(account: &str) -> Result<Entry, KeyringError> {
        let store = zbus_secret_service_keyring_store::Store::new()
            .map_err(|e| KeyringError::Unavailable(e.to_string()))?;
        store
            .build(SERVICE, account, None)
            .map_err(|e| KeyringError::Refused(e.to_string()))
    }

    pub(super) fn refused(error: Error) -> KeyringError {
        match error {
            Error::NoStorageAccess(why) | Error::PlatformFailure(why) => {
                KeyringError::Unavailable(why.to_string())
            }
            other => KeyringError::Refused(other.to_string()),
        }
    }
}

fn token_account(server: &Url, tenant: &str) -> String {
    format!("token {tenant} {}", server.as_str().trim_end_matches('/'))
}

fn save(account: &str, secret: &SecretString) -> Result<(), KeyringError> {
    #[cfg(all(feature = "os-keyring", target_os = "linux"))]
    {
        use oxabl_cache_crypto::ExposeSecret;
        store::entry(account)?
            .set_password(secret.expose_secret())
            .map_err(store::refused)
    }
    #[cfg(not(all(feature = "os-keyring", target_os = "linux")))]
    {
        let _ = (account, secret);
        Err(KeyringError::Unavailable(
            "this build has no keyring".into(),
        ))
    }
}

fn stored(account: &str) -> Option<SecretString> {
    #[cfg(all(feature = "os-keyring", target_os = "linux"))]
    {
        store::entry(account)
            .ok()?
            .get_password()
            .ok()
            .map(SecretString::from)
    }
    #[cfg(not(all(feature = "os-keyring", target_os = "linux")))]
    {
        let _ = account;
        None
    }
}

fn forget(account: &str) -> Result<(), KeyringError> {
    #[cfg(all(feature = "os-keyring", target_os = "linux"))]
    {
        match store::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(store::refused(error)),
        }
    }
    #[cfg(not(all(feature = "os-keyring", target_os = "linux")))]
    {
        let _ = account;
        Err(KeyringError::Unavailable(
            "this build has no keyring".into(),
        ))
    }
}

/// Stores the passphrase of the identity file at `identity_file`, replacing any stored one.
pub fn save_passphrase(
    identity_file: &Path,
    passphrase: &SecretString,
) -> Result<(), KeyringError> {
    save(&account(identity_file), passphrase)
}

/// The stored passphrase of the identity file at `identity_file`; `None` when there is none or
/// no keyring answers.
pub fn stored_passphrase(identity_file: &Path) -> Option<SecretString> {
    stored(&account(identity_file))
}

/// Removes the stored passphrase; removing one that is not there succeeds.
pub fn forget_passphrase(identity_file: &Path) -> Result<(), KeyringError> {
    forget(&account(identity_file))
}

/// Stores an access token for `tenant` on `server`, replacing any stored one.
pub fn save_token(server: &Url, tenant: &str, token: &SecretString) -> Result<(), KeyringError> {
    save(&token_account(server, tenant), token)
}

/// The stored access token for `tenant` on `server`; `None` when there is none or no keyring
/// answers.
pub fn stored_token(server: &Url, tenant: &str) -> Option<SecretString> {
    stored(&token_account(server, tenant))
}

/// Removes the stored access token; removing one that is not there succeeds.
pub fn forget_token(server: &Url, tenant: &str) -> Result<(), KeyringError> {
    forget(&token_account(server, tenant))
}

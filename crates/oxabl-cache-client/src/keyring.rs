//! The identity passphrase in the operating system's keyring, so a person never types it: the
//! Secret Service on Linux. Without the `os-keyring` feature, or on another system, nothing is
//! stored and nothing is found.

use std::path::Path;

use oxabl_cache_crypto::SecretString;

const SERVICE: &str = "oxabl-cache";

/// Why the keyring could not store or remove a passphrase.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyringError {
    /// No keyring is running, or this build has none.
    Unavailable,
    /// The keyring refused or failed.
    Refused(String),
}

impl std::fmt::Display for KeyringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("no keyring is available"),
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
            .map_err(|_| KeyringError::Unavailable)?;
        store
            .build(SERVICE, account, None)
            .map_err(|e| KeyringError::Refused(e.to_string()))
    }

    pub(super) fn refused(error: Error) -> KeyringError {
        match error {
            Error::NoStorageAccess(_) | Error::PlatformFailure(_) => KeyringError::Unavailable,
            other => KeyringError::Refused(other.to_string()),
        }
    }
}

/// Stores the passphrase of the identity file at `identity_file`, replacing any stored one.
pub fn save_passphrase(
    identity_file: &Path,
    passphrase: &SecretString,
) -> Result<(), KeyringError> {
    #[cfg(all(feature = "os-keyring", target_os = "linux"))]
    {
        use oxabl_cache_crypto::ExposeSecret;
        store::entry(&account(identity_file))?
            .set_password(passphrase.expose_secret())
            .map_err(store::refused)
    }
    #[cfg(not(all(feature = "os-keyring", target_os = "linux")))]
    {
        let _ = (account(identity_file), passphrase);
        Err(KeyringError::Unavailable)
    }
}

/// The stored passphrase of the identity file at `identity_file`; `None` when there is none or
/// no keyring answers.
pub fn stored_passphrase(identity_file: &Path) -> Option<SecretString> {
    #[cfg(all(feature = "os-keyring", target_os = "linux"))]
    {
        store::entry(&account(identity_file))
            .ok()?
            .get_password()
            .ok()
            .map(SecretString::from)
    }
    #[cfg(not(all(feature = "os-keyring", target_os = "linux")))]
    {
        let _ = account(identity_file);
        None
    }
}

/// Removes the stored passphrase; removing one that is not there succeeds.
pub fn forget_passphrase(identity_file: &Path) -> Result<(), KeyringError> {
    #[cfg(all(feature = "os-keyring", target_os = "linux"))]
    {
        match store::entry(&account(identity_file))?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(store::refused(error)),
        }
    }
    #[cfg(not(all(feature = "os-keyring", target_os = "linux")))]
    {
        let _ = account(identity_file);
        Err(KeyringError::Unavailable)
    }
}

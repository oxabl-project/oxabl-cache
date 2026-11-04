//! Creating and unlocking member identity files, and the spellings of an identity's public halves.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use oxabl_cache_crypto::{ExposeSecret, SecretString, identity_file};

pub use oxabl_cache_crypto::Identity;

use crate::config::owner_key_text;

/// Why an identity file could not be created or unlocked.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdentityError {
    /// The file already exists; an identity is never overwritten.
    Exists,
    /// The file is missing, is not a regular file, or the passphrase does not unlock it.
    Unlock,
    Io(io::ErrorKind),
    /// The operating system's random generator failed.
    Random,
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exists => f.write_str("the file already exists"),
            Self::Unlock => {
                f.write_str("the identity file cannot be unlocked with that passphrase")
            }
            Self::Io(kind) => write!(f, "i/o: {kind}"),
            Self::Random => f.write_str("the operating system's random generator failed"),
        }
    }
}

impl std::error::Error for IdentityError {}

impl From<io::Error> for IdentityError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::AlreadyExists => Self::Exists,
            kind => Self::Io(kind),
        }
    }
}

/// `$OXABL_CACHE_IDENTITY_FILE`, else `$XDG_CONFIG_HOME/oxabl-cache/identity`, else
/// `~/.config/oxabl-cache/identity`.
pub fn default_identity_path() -> Option<PathBuf> {
    crate::cache::identity::default_file()
}

fn private_parent(path: &Path) -> io::Result<()> {
    match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent),
        None => Ok(()),
    }
}

/// Generates an identity and writes it to `path` as an age file under `passphrase`, with mode
/// 0600. Fails if the file exists.
pub fn create_identity_file(
    path: &Path,
    passphrase: &SecretString,
) -> Result<Identity, IdentityError> {
    let identity = Identity::generate().map_err(|_| IdentityError::Random)?;
    private_parent(path)?;
    identity_file::save(path, &identity, passphrase).map_err(|e| match e {
        oxabl_cache_crypto::Error::Io(io::ErrorKind::AlreadyExists) => IdentityError::Exists,
        oxabl_cache_crypto::Error::Io(kind) => IdentityError::Io(kind),
        _ => IdentityError::Unlock,
    })?;
    Ok(identity)
}

/// Unlocks the identity file at `path`.
pub fn open_identity_file(
    path: &Path,
    passphrase: &SecretString,
) -> Result<Identity, IdentityError> {
    identity_file::load(path, passphrase).map_err(|e| match e {
        oxabl_cache_crypto::Error::Io(kind) => IdentityError::Io(kind),
        _ => IdentityError::Unlock,
    })
}

/// Writes the identity string of `identity` to `path` with mode 0600, for a CI secret store.
/// Fails if the file exists.
pub fn write_identity_secret(path: &Path, identity: &Identity) -> Result<(), IdentityError> {
    let secret = identity
        .to_secret_string()
        .map_err(|_| IdentityError::Random)?;
    private_parent(path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    writeln!(file, "{}", secret.expose_secret())?;
    file.sync_all()?;
    Ok(())
}

/// The identity's signing key in the spelling of `owner_keys`.
pub fn signing_key_text(identity: &Identity) -> String {
    owner_key_text(&identity.signing_public())
}

/// The identity's signing key as lower-case hex, the spelling the tenant API takes for the
/// owner keys of a cache.
pub fn signing_key_hex(identity: &Identity) -> String {
    identity
        .signing_public()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

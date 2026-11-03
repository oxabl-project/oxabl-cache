//! Passphrase-protected identity files: an age file with one scrypt recipient.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use age::secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::identity::Identity;

/// scrypt work factor `log2(N)` of new files: about 1 s to unlock.
pub const WORK_FACTOR: u8 = 18;
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Encrypts the identity under `passphrase` with the standard work factor.
pub fn protect(identity: &Identity, passphrase: &SecretString) -> Result<Vec<u8>> {
    protect_with_work_factor(identity, passphrase, WORK_FACTOR)
}

/// Like [`protect`], with an explicit scrypt `log2(N)` between 1 and 30.
pub fn protect_with_work_factor(
    identity: &Identity,
    passphrase: &SecretString,
    log_n: u8,
) -> Result<Vec<u8>> {
    if !(1..=30).contains(&log_n) {
        return Err(Error::Identity("work factor out of range"));
    }
    let mut recipient = age::scrypt::Recipient::new(passphrase.clone());
    recipient.set_work_factor(log_n);
    let secret = identity.to_secret_string()?;
    age::encrypt(&recipient, secret.expose_secret().as_bytes())
        .map_err(|_| Error::Identity("encryption failed"))
}

/// Unlocks an identity file, refusing one that demands more than the standard work factor.
pub fn unprotect(bytes: &[u8], passphrase: &SecretString) -> Result<Identity> {
    unprotect_with_max_work_factor(bytes, passphrase, WORK_FACTOR)
}

/// Like [`unprotect`], with an explicit ceiling on the file's scrypt `log2(N)`.
pub fn unprotect_with_max_work_factor(
    bytes: &[u8],
    passphrase: &SecretString,
    max_log_n: u8,
) -> Result<Identity> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(Error::Limit("identity file size"));
    }
    let mut unlock = age::scrypt::Identity::new(passphrase.clone());
    unlock.set_max_work_factor(max_log_n);
    let decrypted = Zeroizing::new(
        age::decrypt(&unlock, bytes)
            .map_err(|_| Error::Identity("cannot unlock the identity file"))?,
    );
    let text = std::str::from_utf8(&decrypted).map_err(|_| Error::Identity("not an identity"))?;
    Identity::parse(text)
}

/// Writes a protected identity file with mode 0600. Fails if the path exists.
pub fn save(path: &Path, identity: &Identity, passphrase: &SecretString) -> Result<()> {
    let bytes = protect(identity, passphrase)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Reads and unlocks a protected identity file.
pub fn load(path: &Path, passphrase: &SecretString) -> Result<Identity> {
    let file = File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err(Error::Identity("not a regular identity file"));
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_FILE_BYTES).read_to_end(&mut bytes)?;
    unprotect(&bytes, passphrase)
}

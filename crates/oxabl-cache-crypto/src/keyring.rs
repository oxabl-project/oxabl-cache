//! The keyring: the root key of an epoch, age-encrypted to every member's recipients.

use std::io::Read;
use std::iter;
use std::str::FromStr;

use oxabl_cache_format::CacheId;
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::keys::RootKey;

const PLAINTEXT_LEN: usize = 32 + 16 + 4;

fn plaintext(root: &RootKey, cache_id: &CacheId, epoch: u32) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::with_capacity(PLAINTEXT_LEN));
    out.extend_from_slice(root.bytes());
    out.extend_from_slice(&cache_id.0);
    out.extend_from_slice(&epoch.to_be_bytes());
    out
}

/// Encrypts `CRK ‖ cache_id ‖ epoch` to every recipient string.
pub(crate) fn wrap<'a>(
    root: &RootKey,
    cache_id: &CacheId,
    epoch: u32,
    recipients: impl Iterator<Item = &'a str>,
) -> Result<Vec<u8>> {
    let recipients = recipients
        .map(|r| {
            age::x25519::Recipient::from_str(r).map_err(|_| Error::Keyring("malformed recipient"))
        })
        .collect::<Result<Vec<_>>>()?;
    let encryptor =
        age::Encryptor::with_recipients(recipients.iter().map(|r| r as &dyn age::Recipient))
            .map_err(|_| Error::Keyring("no recipients"))?;
    let mut out = Vec::new();
    let mut writer = encryptor
        .wrap_output(&mut out)
        .map_err(|_| Error::Keyring("encryption failed"))?;
    std::io::Write::write_all(&mut writer, &plaintext(root, cache_id, epoch))
        .map_err(|_| Error::Keyring("encryption failed"))?;
    writer
        .finish()
        .map_err(|_| Error::Keyring("encryption failed"))?;
    Ok(out)
}

/// Decrypts the keyring with `identity` and checks it names this cache and epoch.
pub(crate) fn unwrap(
    keyring: &[u8],
    identity: &Identity,
    cache_id: &CacheId,
    epoch: u32,
) -> Result<RootKey> {
    let decryptor =
        age::Decryptor::new(keyring).map_err(|_| Error::Keyring("malformed keyring"))?;
    let mut reader = decryptor
        .decrypt(iter::once(identity.age_identity() as &dyn age::Identity))
        .map_err(|_| Error::Keyring("not encrypted to this identity"))?;
    let mut buf = Zeroizing::new(Vec::with_capacity(PLAINTEXT_LEN + 1));
    reader
        .by_ref()
        .take(PLAINTEXT_LEN as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|_| Error::Keyring("corrupt keyring"))?;
    if buf.len() != PLAINTEXT_LEN {
        return Err(Error::Keyring("wrong length"));
    }
    if buf[32..48] != cache_id.0 || buf[48..52] != epoch.to_be_bytes() {
        return Err(Error::Keyring("names another cache or epoch"));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&buf[..32]);
    let root = RootKey::from_bytes(key);
    zeroize::Zeroize::zeroize(&mut key);
    Ok(root)
}

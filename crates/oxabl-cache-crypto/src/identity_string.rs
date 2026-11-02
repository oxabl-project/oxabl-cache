//! The Bech32 forms of an identity and its public descriptor.

use oxabl_cache_format::bech32::{self, IDENTITY_HRP};
use oxabl_cache_format::{FormatError, MemberDescriptor};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

const PAYLOAD_VERSION: u8 = 1;
const PAYLOAD_LEN: usize = 1 + 32 + 32;
const AGE_PUBLIC_HRP: &str = "age";
const AGE_SECRET_HRP: &str = "age-secret-key-";

/// Both secrets of an identity.
pub(crate) struct IdentitySecrets {
    pub(crate) x25519: Zeroizing<[u8; 32]>,
    pub(crate) ed25519_seed: Zeroizing<[u8; 32]>,
}

/// The identity string: the upper-case Bech32 form, which is how it is shown and stored.
pub(crate) fn encode_identity(secrets: &IdentitySecrets) -> Result<Zeroizing<String>> {
    let mut payload = Zeroizing::new(Vec::with_capacity(PAYLOAD_LEN));
    payload.push(PAYLOAD_VERSION);
    payload.extend_from_slice(secrets.x25519.as_slice());
    payload.extend_from_slice(secrets.ed25519_seed.as_slice());
    let lower = Zeroizing::new(bech32::encode(IDENTITY_HRP, &payload)?);
    Ok(Zeroizing::new(lower.to_ascii_uppercase()))
}

fn lowercased(s: &str) -> Result<Zeroizing<String>> {
    let has_lower = s.bytes().any(|c| c.is_ascii_lowercase());
    let has_upper = s.bytes().any(|c| c.is_ascii_uppercase());
    if has_lower && has_upper {
        return Err(Error::Identity("mixed-case string"));
    }
    Ok(Zeroizing::new(s.to_ascii_lowercase()))
}

pub(crate) fn decode_identity(s: &str) -> Result<IdentitySecrets> {
    let lower = lowercased(s.trim())?;
    let payload = Zeroizing::new(
        bech32::decode(&lower, IDENTITY_HRP)
            .map_err(|_| Error::Identity("not an identity string"))?,
    );
    let [version, secrets @ ..] = payload.as_slice() else {
        return Err(Error::Identity("not an identity string"));
    };
    if *version != PAYLOAD_VERSION {
        return Err(Error::Format(FormatError::UnsupportedVersion(*version)));
    }
    let secrets: &[u8; 64] = secrets
        .try_into()
        .map_err(|_| Error::Identity("wrong identity length"))?;
    let mut x25519 = Zeroizing::new([0u8; 32]);
    let mut ed25519_seed = Zeroizing::new([0u8; 32]);
    x25519.copy_from_slice(&secrets[..32]);
    ed25519_seed.copy_from_slice(&secrets[32..]);
    Ok(IdentitySecrets {
        x25519,
        ed25519_seed,
    })
}

/// The `AGE-SECRET-KEY-1…` string of an X25519 secret, which is the only way into `age`.
pub(crate) fn age_secret_string(x25519: &[u8; 32]) -> Result<Zeroizing<String>> {
    let lower = Zeroizing::new(bech32::encode(AGE_SECRET_HRP, x25519)?);
    Ok(Zeroizing::new(lower.to_ascii_uppercase()))
}

/// The `age1…` recipient string of an X25519 public key.
pub(crate) fn age_recipient_string(x25519_public: &[u8; 32]) -> Result<String> {
    Ok(bech32::encode(AGE_PUBLIC_HRP, x25519_public)?)
}

/// The X25519 public key inside an `age1…` recipient string.
pub(crate) fn age_recipient_bytes(recipient: &str) -> Result<[u8; 32]> {
    bech32::decode(recipient, AGE_PUBLIC_HRP)?
        .try_into()
        .map_err(|_| Error::Identity("wrong recipient length"))
}

/// Parses a member descriptor, which may be upper case.
pub fn parse_descriptor(s: &str) -> Result<MemberDescriptor> {
    let lower = lowercased(s.trim())?;
    Ok(MemberDescriptor::decode(&lower)?)
}

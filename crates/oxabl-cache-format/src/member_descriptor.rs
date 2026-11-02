//! Public member descriptor string, as `member add` takes it.

use crate::bech32;
use crate::error::{FormatError, Result};

/// HRP of a member descriptor string.
pub const MEMBER_HRP: &str = "oxabl-cache-member-";
const PAYLOAD_VERSION: u8 = 1;

/// A member's public keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberDescriptor {
    pub ed25519_public: [u8; 32],
    pub x25519_public: [u8; 32],
}

impl MemberDescriptor {
    pub fn encode(&self) -> Result<String> {
        let mut payload = Vec::with_capacity(65);
        payload.push(PAYLOAD_VERSION);
        payload.extend_from_slice(&self.ed25519_public);
        payload.extend_from_slice(&self.x25519_public);
        bech32::encode(MEMBER_HRP, &payload)
    }

    pub fn decode(s: &str) -> Result<Self> {
        let payload = bech32::decode(s, MEMBER_HRP)?;
        let [version, keys @ ..] = payload.as_slice() else {
            return Err(FormatError::Truncated);
        };
        if *version != PAYLOAD_VERSION {
            return Err(FormatError::UnsupportedVersion(*version));
        }
        let keys: &[u8; 64] = keys
            .try_into()
            .map_err(|_| FormatError::Invalid("descriptor length"))?;
        let (ed, x) = keys.split_at(32);
        Ok(Self {
            ed25519_public: ed.try_into().map_err(|_| FormatError::Truncated)?,
            x25519_public: x.try_into().map_err(|_| FormatError::Truncated)?,
        })
    }
}

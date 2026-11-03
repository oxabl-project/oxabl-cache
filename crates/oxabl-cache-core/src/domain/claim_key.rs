//! The key that turns policy claim values into stored hashes.

use crate::ids::TenantId;
use std::fmt;

/// 32 secret bytes; the value never appears in `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct ClaimKey([u8; 32]);

impl ClaimKey {
    /// Wraps the key bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The keyed hash of one claim value, bound to the tenant, the issuer and the claim name.
    pub fn hash(&self, tenant: &TenantId, issuer: &str, claim: &str, value: &str) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new_keyed(&self.0);
        hasher.update(tenant.as_bytes());
        for part in [issuer, claim, value] {
            hasher.update(&(part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

impl fmt::Debug for ClaimKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClaimKey(..)")
    }
}

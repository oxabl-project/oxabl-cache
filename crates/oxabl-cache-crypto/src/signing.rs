use ed25519_dalek::{Signature, VerifyingKey};
use oxabl_cache_format::{PublicKey, Signature as SignatureBytes};

use crate::error::{Error, Result};

/// Verifies a strict Ed25519 signature: canonical `S`, no small-order or invalid keys.
pub(crate) fn verify(public: &PublicKey, message: &[u8], signature: &SignatureBytes) -> Result<()> {
    let key = VerifyingKey::from_bytes(public).map_err(|_| Error::Signature)?;
    key.verify_strict(message, &Signature::from_bytes(signature))
        .map_err(|_| Error::Signature)
}

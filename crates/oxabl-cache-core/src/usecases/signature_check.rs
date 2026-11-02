//! Ed25519 verification over public keys; the server holds no secret key.

use crate::domain::roster::{PublicKey, SignatureBytes};
use ed25519_dalek::{Signature, VerifyingKey};

/// Whether `signature` is a valid strict Ed25519 signature of `message` under `key`.
pub(crate) fn verifies(key: &PublicKey, message: &[u8], signature: &SignatureBytes) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(key) else {
        return false;
    };
    key.verify_strict(message, &Signature::from_bytes(signature))
        .is_ok()
}

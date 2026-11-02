//! The textual form of tokens and sessions: `<prefix><id>_<secret>`.

use crate::domain::credential::CredentialKind;
use crate::encoding::{base32_decode, base32_encode};
use crate::ids::CredentialId;
use sha2::{Digest, Sha256};

const SECRET_BYTES: usize = 32;
const OPERATOR_PREFIX: &str = "oxo_";

/// A credential split into its parts.
pub(crate) struct ParsedCredential {
    pub kind: CredentialKind,
    pub id: CredentialId,
    pub secret: [u8; SECRET_BYTES],
}

/// SHA-256 of a secret, the only form that is stored.
pub(crate) fn hash_secret(secret: &[u8; SECRET_BYTES]) -> [u8; 32] {
    let digest = Sha256::digest(secret);
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&digest);
    hash
}

/// A fresh id and secret, and the text that is shown once.
pub(crate) fn generate(kind: CredentialKind) -> (CredentialId, [u8; SECRET_BYTES], String) {
    let id = CredentialId::random();
    let mut secret = [0u8; SECRET_BYTES];
    getrandom::fill(&mut secret).expect("the operating system provides randomness");
    let text = format!("{}{}_{}", kind.prefix(), id, base32_encode(&secret));
    (id, secret, text)
}

/// Splits credential text; anything not in canonical form is `None`.
pub(crate) fn parse(text: &str) -> Option<ParsedCredential> {
    let (kind, rest) = [CredentialKind::Token, CredentialKind::Session]
        .into_iter()
        .find_map(|kind| text.strip_prefix(kind.prefix()).map(|rest| (kind, rest)))?;
    let (id, secret) = rest.split_once('_')?;
    Some(ParsedCredential {
        kind,
        id: id.parse().ok()?,
        secret: base32_decode(secret)?,
    })
}

/// An operator credential split into its parts.
pub(crate) struct ParsedOperator {
    pub id: CredentialId,
    pub secret: [u8; SECRET_BYTES],
}

/// A fresh operator id and secret, and the text that is shown once.
pub(crate) fn generate_operator() -> (CredentialId, [u8; SECRET_BYTES], String) {
    let id = CredentialId::random();
    let mut secret = [0u8; SECRET_BYTES];
    getrandom::fill(&mut secret).expect("the operating system provides randomness");
    let text = format!("{OPERATOR_PREFIX}{id}_{}", base32_encode(&secret));
    (id, secret, text)
}

/// Splits operator credential text; anything not in canonical form is `None`.
pub(crate) fn parse_operator(text: &str) -> Option<ParsedOperator> {
    let (id, secret) = text.strip_prefix(OPERATOR_PREFIX)?.split_once('_')?;
    Some(ParsedOperator {
        id: id.parse().ok()?,
        secret: base32_decode(secret)?,
    })
}

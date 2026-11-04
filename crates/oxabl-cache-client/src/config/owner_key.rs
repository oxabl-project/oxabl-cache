use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use oxabl_cache_format::PublicKey;

use super::error::ConfigError;

const PREFIX: &str = "ed25519:";

/// Parses `ed25519:` and the canonical unpadded standard base64 of a 32-byte public key.
pub(crate) fn parse_owner_key(text: &str) -> Result<PublicKey, ConfigError> {
    let invalid = ConfigError::Invalid("owner_keys holds `ed25519:` and unpadded base64");
    let encoded = text.strip_prefix(PREFIX).ok_or(invalid.clone())?;
    let bytes = STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|_| invalid.clone())?;
    let key = PublicKey::try_from(bytes).map_err(|_| invalid.clone())?;
    if STANDARD_NO_PAD.encode(key) != encoded {
        return Err(invalid);
    }
    Ok(key)
}

/// The `owner_keys` spelling of a public key: `ed25519:` and unpadded standard base64.
pub(crate) fn owner_key_text(key: &PublicKey) -> String {
    format!("{PREFIX}{}", STANDARD_NO_PAD.encode(key))
}

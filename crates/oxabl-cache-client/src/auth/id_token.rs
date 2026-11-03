use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use oxabl_cache_crypto::{ExposeSecret, SecretString};
use oxabl_cache_format::TenantId;

use crate::error::IdTokenError;

/// The audience every ID token must carry, derived from the committed tenant.
pub(crate) fn audience(tenant: &TenantId) -> String {
    format!("oxabl-cache:{}", tenant.to_text())
}

/// Refuses an ID token unless its `aud` is exactly `expected`. The signature is the server's
/// to check; this only keeps a token minted for another audience from leaving the process.
pub(crate) fn check_audience(token: &SecretString, expected: &str) -> Result<(), IdTokenError> {
    let mut parts = token.expose_secret().split('.');
    let (_, Some(payload), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(IdTokenError::Malformed);
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| IdTokenError::Malformed)?;
    let claims: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| IdTokenError::Malformed)?;
    match claims.get("aud").and_then(serde_json::Value::as_str) {
        Some(aud) if aud == expected => Ok(()),
        _ => Err(IdTokenError::AudienceMismatch),
    }
}

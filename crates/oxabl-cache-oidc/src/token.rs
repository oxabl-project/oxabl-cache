//! Parsing and checking the claims of an ID token.

use crate::jwk::Algorithm;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Map, Value};

/// Largest accepted token.
pub(crate) const MAX_TOKEN_BYTES: usize = 16 * 1024;
/// Allowed clock difference in seconds.
pub(crate) const SKEW_SECS: i64 = 60;

/// A token split into its parts, signature not yet checked.
pub(crate) struct Parsed<'t> {
    pub algorithm: Algorithm,
    pub kid: Option<String>,
    pub claims: Map<String, Value>,
    pub signed: &'t str,
    pub signature: Vec<u8>,
}

fn object(segment: &str) -> Option<Map<String, Value>> {
    match serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segment).ok()?).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Splits a compact JWT and reads its header and payload.
pub(crate) fn parse(token: &str) -> Option<Parsed<'_>> {
    if token.len() > MAX_TOKEN_BYTES {
        return None;
    }
    let mut parts = token.split('.');
    let (header, payload, signature) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let header = object(header)?;
    if header.contains_key("crit") {
        return None;
    }
    let algorithm = Algorithm::parse(header.get("alg")?.as_str()?)?;
    let kid = match header.get("kid") {
        None => None,
        Some(Value::String(kid)) => Some(kid.clone()),
        Some(_) => return None,
    };
    Some(Parsed {
        algorithm,
        kid,
        claims: object(payload)?,
        signed: &token[..token.len() - signature.len() - 1],
        signature: URL_SAFE_NO_PAD.decode(signature).ok()?,
    })
}

fn seconds(claims: &Map<String, Value>, name: &str) -> Option<Option<i64>> {
    match claims.get(name) {
        None => Some(None),
        Some(value) => value.as_f64().map(|v| Some(v.floor() as i64)),
    }
}

/// Whether `iss` is the given issuer, `aud` is exactly `audience`, and `exp`, `nbf`
/// and `iat` hold within the skew at `now`. `exp` and `iat` are required.
pub(crate) fn claims_hold(
    claims: &Map<String, Value>,
    issuer: &str,
    audience: &str,
    now: i64,
) -> bool {
    let audience_ok = match claims.get("aud") {
        Some(Value::String(aud)) => aud == audience,
        Some(Value::Array(list)) => {
            matches!(list.as_slice(), [Value::String(aud)] if aud == audience)
        }
        _ => false,
    };
    let (Some(exp), Some(nbf), Some(iat)) = (
        seconds(claims, "exp").flatten(),
        seconds(claims, "nbf"),
        seconds(claims, "iat").flatten(),
    ) else {
        return false;
    };
    claims.get("iss").and_then(Value::as_str) == Some(issuer)
        && audience_ok
        && now <= exp.saturating_add(SKEW_SECS)
        && nbf.is_none_or(|nbf| nbf <= now + SKEW_SECS)
        && iat <= now + SKEW_SECS
}

/// The scalar claims a trust policy may match: strings, whole numbers and booleans.
pub(crate) fn scalar_claims(claims: &Map<String, Value>) -> Vec<(String, String)> {
    claims
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "exp" | "nbf" | "iat" | "aud"))
        .filter_map(|(name, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Bool(flag) => flag.to_string(),
                Value::Number(number) if number.is_i64() || number.is_u64() => number.to_string(),
                _ => return None,
            };
            Some((name.clone(), text))
        })
        .collect()
}

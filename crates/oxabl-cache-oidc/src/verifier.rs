//! The `IdentityVerifier` of the server.

use crate::fetch::Fetcher;
use crate::jwk::Jwk;
use crate::keyset::{KeySets, fetch_keys};
use crate::settings::OidcSettings;
use crate::token::{self, claims_hold, scalar_claims};
use oxabl_cache_core::ids::TenantId;
use oxabl_cache_core::ports::{IdentityVerifier, VerifiedIdentity};
use oxabl_cache_core::{Error, Result};
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const REFUSED: &str = "the issuer could not be verified";

/// Verifies OIDC ID tokens against tenant issuer allowlists, with every key fetch
/// guarded against server-side request forgery.
pub struct OidcVerifier {
    fetcher: Arc<Fetcher>,
    keys: KeySets,
}

impl OidcVerifier {
    /// Builds a verifier; fails when the extra CA file does not parse.
    pub fn new(settings: OidcSettings) -> std::result::Result<Self, String> {
        let fetcher = Arc::new(Fetcher::new(settings)?);
        Ok(Self {
            keys: KeySets::new(Arc::clone(&fetcher)),
            fetcher,
        })
    }

    fn allowed(&self, issuer: &str) -> bool {
        let list = &self.fetcher.settings().issuer_allowlist;
        list.is_empty() || list.iter().any(|allowed| allowed == issuer)
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

fn signed_by(keys: &[Jwk], parsed: &token::Parsed<'_>) -> bool {
    let candidates: Vec<&Jwk> = keys
        .iter()
        .filter(|key| key.fits(parsed.algorithm))
        .collect();
    let chosen: Vec<&Jwk> = match parsed.kid.as_deref() {
        Some(kid) => candidates
            .into_iter()
            .filter(|key| key.kid() == Some(kid))
            .collect(),
        None if candidates.len() == 1 => candidates,
        None => Vec::new(),
    };
    chosen
        .iter()
        .any(|key| key.verify(parsed.signed.as_bytes(), &parsed.signature))
}

impl IdentityVerifier for OidcVerifier {
    async fn verify(
        &self,
        tenant: &TenantId,
        issuers: &[String],
        id_token: &str,
    ) -> Result<VerifiedIdentity> {
        let parsed = token::parse(id_token).ok_or(Error::Unauthenticated)?;
        let issuer = parsed
            .claims
            .get("iss")
            .and_then(Value::as_str)
            .filter(|iss| issuers.iter().any(|known| known == iss) && self.allowed(iss))
            .ok_or(Error::Unauthenticated)?
            .to_owned();
        let keys = self
            .keys
            .keys(&issuer, parsed.kid.as_deref())
            .await
            .map_err(|_| Error::unavailable("issuer keys are unavailable"))?;
        if !signed_by(&keys, &parsed) {
            return Err(Error::Unauthenticated);
        }
        let audience = format!("oxabl-cache:{tenant}");
        if !claims_hold(&parsed.claims, &issuer, &audience, now()) {
            return Err(Error::Unauthenticated);
        }
        let subject = parsed
            .claims
            .get("sub")
            .and_then(Value::as_str)
            .filter(|sub| !sub.is_empty())
            .ok_or(Error::Unauthenticated)?
            .to_owned();
        Ok(VerifiedIdentity {
            issuer,
            subject,
            claims: scalar_claims(&parsed.claims),
        })
    }

    async fn probe_issuer(&self, issuer: &str) -> Result<()> {
        if !self.allowed(issuer) {
            return Err(Error::bad_request(REFUSED));
        }
        fetch_keys(&self.fetcher, issuer)
            .await
            .map(|_| ())
            .map_err(|_| Error::bad_request(REFUSED))
    }
}

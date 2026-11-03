//! Operator settings of the verifier.

use crate::cidr::Cidr;
use crate::dns::Resolver;

/// Operator settings for issuer fetches and registration.
#[derive(Debug, Clone)]
pub struct OidcSettings {
    /// When not empty, only these issuers may be registered or verified.
    pub issuer_allowlist: Vec<String>,
    /// Private ranges an issuer may resolve into.
    pub allowed_private_ranges: Vec<Cidr>,
    /// How issuer names are resolved.
    pub resolver: Resolver,
    /// PEM certificates trusted for issuers in addition to the system roots.
    pub extra_ca_pem: Option<Vec<u8>>,
}

impl Default for OidcSettings {
    fn default() -> Self {
        Self {
            issuer_allowlist: Vec::new(),
            allowed_private_ranges: Vec::new(),
            resolver: Resolver::System,
            extra_ca_pem: None,
        }
    }
}

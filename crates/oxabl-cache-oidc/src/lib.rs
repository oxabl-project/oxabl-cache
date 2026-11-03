//! `IdentityVerifier` adapter: ID-token validation (RS256, ES256, EdDSA), issuer key
//! sets and the guard that keeps every issuer fetch away from internal addresses.

mod address;
mod cidr;
mod dns;
mod fetch;
mod jwk;
mod keyset;
mod settings;
mod target;
mod token;
mod verifier;

pub use cidr::Cidr;
pub use dns::Resolver;
pub use settings::OidcSettings;
pub use verifier::OidcVerifier;

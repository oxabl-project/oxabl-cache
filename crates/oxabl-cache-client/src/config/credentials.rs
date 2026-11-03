use std::path::PathBuf;

use oxabl_cache_crypto::SecretString;

/// Where the client gets its server credential.
#[derive(Debug, Clone, Default)]
pub enum Credentials {
    /// `OXABL_CACHE_TOKEN`, then GitHub Actions OIDC, then GitLab `OXABL_CACHE_ID_TOKEN`, then
    /// the stored login session.
    #[default]
    Auto,
    /// An API or session token, sent as the bearer.
    Token(SecretString),
    /// An OIDC ID token, exchanged for a session once its audience is checked.
    OidcIdToken(SecretString),
}

/// Where the client gets its member identity.
#[derive(Debug, Clone, Default)]
pub enum IdentitySource {
    /// `OXABL_CACHE_IDENTITY`, then the identity file.
    #[default]
    Auto,
    /// An identity string.
    Value(SecretString),
    /// A passphrase-protected identity file.
    File(PathBuf),
}

//! The errors of opening a cache and of operating on one.

use std::fmt;
use std::io;

use crate::config::ConfigError;

/// Why an ID token is not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdTokenError {
    /// The token is not a JWT with a JSON payload.
    Malformed,
    /// The audience is not exactly `oxabl-cache:<tenant>`.
    AudienceMismatch,
    /// The CI system's token endpoint did not answer with a token.
    IssuerUnavailable,
}

impl fmt::Display for IdTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "the ID token is malformed",
            Self::AudienceMismatch => "the ID token's audience is not oxabl-cache:<tenant>",
            Self::IssuerUnavailable => "the CI system gave no ID token",
        })
    }
}

/// Why a cache could not be opened. oxabl reports it and builds without the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    Config(ConfigError),
    /// No credential source produced a credential.
    NoCredentials,
    /// The identity is missing, malformed or could not be unlocked.
    Identity(&'static str),
    IdToken(IdTokenError),
    /// The server could not be reached.
    Unreachable,
    /// The server refused the credential.
    Authentication,
    /// The cache does not exist or the credential has no grant on it.
    CacheNotFound,
    /// The server answered with an unexpected status.
    Server(u16),
    /// The server's answer is not what the protocol defines.
    BadResponse,
    /// The cache has no roster yet.
    NoRoster,
    /// The roster failed verification: signature, pin, rollback or malformed content.
    Roster(oxabl_cache_crypto::Error),
    /// The identity is not a member of the roster.
    IdentityNotInRoster,
    /// Local state could not be read.
    State(io::ErrorKind),
}

impl OpenError {
    /// Whether retrying later may succeed, so the caller shows a warning, not an error.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Unreachable => true,
            Self::Server(status) => *status == 429 || *status >= 500,
            Self::IdToken(IdTokenError::IssuerUnavailable) => true,
            _ => false,
        }
    }
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(e) => e.fmt(f),
            Self::NoCredentials => f.write_str("no credentials: set OXABL_CACHE_TOKEN or log in"),
            Self::Identity(what) => write!(f, "identity: {what}"),
            Self::IdToken(e) => e.fmt(f),
            Self::Unreachable => f.write_str("the cache server is unreachable"),
            Self::Authentication => f.write_str("the cache server refused the credentials"),
            Self::CacheNotFound => f.write_str("the cache does not exist or is not granted"),
            Self::Server(status) => write!(f, "the cache server answered {status}"),
            Self::BadResponse => f.write_str("the cache server sent a malformed answer"),
            Self::NoRoster => f.write_str("the cache has no roster yet"),
            Self::Roster(e) => write!(f, "roster: {e}"),
            Self::IdentityNotInRoster => f.write_str("the identity is not in the roster"),
            Self::State(kind) => write!(f, "local state: {kind}"),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<ConfigError> for OpenError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

/// Why an operation on an open cache failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CacheError {
    /// The mode does not write.
    ReadOnly,
    /// A `ci` mode client whose identity is not a `ci` member of the roster.
    NotAWriter,
    /// The server is unreachable or answered an error.
    Unavailable,
    /// A sample must hold 1 to 1,000 entries.
    SampleSize,
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ReadOnly => "the mode does not write to the cache",
            Self::NotAWriter => "the identity is not a ci member of the roster",
            Self::Unavailable => "the cache server is unavailable",
            Self::SampleSize => "a sample holds 1 to 1,000 entries",
        })
    }
}

impl std::error::Error for CacheError {}

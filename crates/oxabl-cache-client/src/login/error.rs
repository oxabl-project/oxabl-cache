use std::fmt;
use std::io;

use crate::config::ConfigError;
use crate::error::{IdTokenError, OpenError};

/// Why a login or logout did not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoginError {
    Config(ConfigError),
    /// The issuer's discovery document is missing, unreachable or inconsistent.
    Issuer(&'static str),
    /// The identity provider or the cache server could not be reached.
    Unreachable,
    /// The user denied the request.
    Denied,
    /// The code expired before it was approved.
    Expired,
    /// The identity provider refused the request or answered something the grant does not allow.
    Protocol(&'static str),
    /// The ID token cannot be sent to the cache server.
    IdToken(IdTokenError),
    /// The cache server did not open a session for the ID token.
    Exchange(OpenError),
    State(io::ErrorKind),
}

impl fmt::Display for LoginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(e) => e.fmt(f),
            Self::Issuer(what) => write!(f, "issuer: {what}"),
            Self::Unreachable => f.write_str("the identity provider is unreachable"),
            Self::Denied => f.write_str("the login was denied"),
            Self::Expired => f.write_str("the login code expired before it was approved"),
            Self::Protocol(what) => write!(f, "identity provider: {what}"),
            Self::IdToken(e) => e.fmt(f),
            Self::Exchange(e) => write!(f, "the cache server did not open a session: {e}"),
            Self::State(kind) => write!(f, "local state: {kind}"),
        }
    }
}

impl std::error::Error for LoginError {}

impl From<ConfigError> for LoginError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

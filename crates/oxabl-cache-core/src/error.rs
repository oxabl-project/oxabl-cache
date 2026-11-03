//! The error every port and use case reports.

use std::fmt;

/// A refusal a caller can act on, or an infrastructure failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The credential is missing, unknown, expired or revoked.
    Unauthenticated,
    /// The caller lacks the scope or is not the principal the action needs.
    Forbidden(&'static str),
    /// The tenant of the credential is suspended.
    TenantSuspended,
    /// The object does not exist, or belongs to another tenant.
    NotFound,
    /// The request clashes with recorded state.
    Conflict(Conflict),
    /// An `If-Match` version is stale.
    PreconditionFailed,
    /// The request is malformed or out of range.
    BadRequest(String),
    /// The request exceeds a size or count limit.
    TooLarge,
    /// A quota refuses the request.
    QuotaExceeded,
    /// The caller exceeded a rate limit.
    RateLimited { retry_after_secs: u64 },
    /// The metadata store or the object store failed.
    Unavailable(String),
    /// A recorded invariant does not hold.
    Internal(String),
    /// The feature needs settings this deployment lacks.
    NotConfigured(&'static str),
}

/// The reason for a [`Error::Conflict`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// A natural key exists with different content.
    Exists,
    /// The pack is no longer open for the request.
    PackClosed,
    /// The stored object differs from the one recorded at commit.
    PackChanged,
    /// The stored object is missing or its size differs from the declared one.
    PackIncomplete,
}

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Builds a [`Error::BadRequest`].
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    /// Builds an [`Error::Internal`].
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Builds an [`Error::Unavailable`].
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthenticated => f.write_str("unauthenticated"),
            Self::Forbidden(why) => write!(f, "forbidden: {why}"),
            Self::TenantSuspended => f.write_str("tenant suspended"),
            Self::NotFound => f.write_str("not found"),
            Self::Conflict(kind) => write!(f, "conflict: {kind:?}"),
            Self::PreconditionFailed => f.write_str("precondition failed"),
            Self::BadRequest(why) => write!(f, "bad request: {why}"),
            Self::TooLarge => f.write_str("too large"),
            Self::QuotaExceeded => f.write_str("quota exceeded"),
            Self::RateLimited { retry_after_secs } => {
                write!(f, "rate limited, retry after {retry_after_secs}s")
            }
            Self::Unavailable(why) => write!(f, "unavailable: {why}"),
            Self::Internal(why) => write!(f, "internal: {why}"),
            Self::NotConfigured(what) => write!(f, "not configured: {what}"),
        }
    }
}

impl std::error::Error for Error {}

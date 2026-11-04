use std::fmt;
use std::io;

use crate::api::ApiError;
use crate::error::OpenError;

/// Why a roster change was not published.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OwnerError {
    Open(OpenError),
    /// The identity is not an `owner` member of the roster whose signing key the repository pins.
    NotAnOwner,
    /// An input is malformed or breaks a rule of the roster.
    Invalid(&'static str),
    NoSuchMember,
    /// The change would leave the roster without an owner.
    LastOwner,
    /// The change would remove the identity that signs it.
    RemovesSigner,
    /// The roster already exists; it cannot be created again.
    AlreadyInitialised,
    /// Someone else published a roster first: read it again and repeat the change.
    Stale,
    /// The server refused the credential, which needs `cache:admin`.
    Authentication,
    /// The server answered an error.
    Refused {
        status: u16,
        code: Option<String>,
    },
    Unreachable,
    /// The server's answer is not what the protocol defines.
    Malformed,
    Crypto(oxabl_cache_crypto::Error),
    State(io::ErrorKind),
}

impl OwnerError {
    pub(crate) fn from_api(error: ApiError) -> Self {
        match error {
            ApiError::Transport => Self::Unreachable,
            ApiError::Malformed => Self::Malformed,
            ApiError::Status(e) => match e.status {
                401 | 403 => Self::Authentication,
                412 => Self::Stale,
                status => Self::Refused {
                    status,
                    code: e.code,
                },
            },
        }
    }
}

impl fmt::Display for OwnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(e) => e.fmt(f),
            Self::NotAnOwner => f.write_str(
                "the identity is not an owner of the roster, or its signing key is not in owner_keys",
            ),
            Self::Invalid(what) => write!(f, "invalid input: {what}"),
            Self::NoSuchMember => f.write_str("the roster has no such member"),
            Self::LastOwner => f.write_str("the roster would be left without an owner"),
            Self::RemovesSigner => f.write_str("another owner must remove the identity that signs"),
            Self::AlreadyInitialised => f.write_str("the cache already has a roster"),
            Self::Stale => f.write_str("another roster was published first; repeat the change"),
            Self::Authentication => {
                f.write_str("the server refused the credential: publishing a roster needs cache:admin")
            }
            Self::Refused { status, code } => match code {
                Some(code) => write!(f, "the server refused the roster: {status} {code}"),
                None => write!(f, "the server refused the roster: {status}"),
            },
            Self::Unreachable => f.write_str("the cache server is unreachable"),
            Self::Malformed => f.write_str("the cache server sent a malformed answer"),
            Self::Crypto(e) => write!(f, "{e}"),
            Self::State(kind) => write!(f, "local state: {kind}"),
        }
    }
}

impl std::error::Error for OwnerError {}

impl From<OpenError> for OwnerError {
    fn from(e: OpenError) -> Self {
        Self::Open(e)
    }
}

impl From<oxabl_cache_crypto::Error> for OwnerError {
    fn from(e: oxabl_cache_crypto::Error) -> Self {
        Self::Crypto(e)
    }
}

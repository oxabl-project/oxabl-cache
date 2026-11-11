//! Failures and the exit code each one maps to.

use std::fmt;

use oxabl_cache_client::{OpenError, OwnerError, TenantError};

pub const EXIT_REFUSED: u8 = 1;
pub const EXIT_USAGE: u8 = 2;

#[derive(Debug)]
pub enum Failure {
    /// The command was understood but refused or failed.
    Refused(String),
    /// The arguments, the environment or the configuration are wrong.
    Usage(String),
}

impl Failure {
    pub fn refused(message: impl fmt::Display) -> Self {
        Self::Refused(message.to_string())
    }

    pub fn usage(message: impl fmt::Display) -> Self {
        Self::Usage(message.to_string())
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Refused(_) => EXIT_REFUSED,
            Self::Usage(_) => EXIT_USAGE,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(message) | Self::Usage(message) => f.write_str(message),
        }
    }
}

impl From<OpenError> for Failure {
    fn from(error: OpenError) -> Self {
        match error {
            OpenError::Config(_) | OpenError::NoCredentials => Self::usage(error),
            other => Self::refused(other),
        }
    }
}

impl From<OwnerError> for Failure {
    fn from(error: OwnerError) -> Self {
        match error {
            OwnerError::Open(open) => open.into(),
            OwnerError::Invalid(_) => Self::usage(error),
            other => Self::refused(other),
        }
    }
}

impl From<TenantError> for Failure {
    fn from(error: TenantError) -> Self {
        match error {
            TenantError::Open(open) => open.into(),
            TenantError::Invalid(_) => Self::usage(error),
            other => Self::refused(other),
        }
    }
}

//! Failures and the exit code each one maps to.

use std::fmt;

pub const EXIT_REFUSED: u8 = 1;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_UNREACHABLE: u8 = 3;

#[derive(Debug)]
pub enum Failure {
    Usage(String),
    Refused {
        status: u16,
        code: String,
        message: String,
    },
    Unreachable(String),
}

impl Failure {
    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    pub fn refused(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self::Refused {
            status,
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Refused { .. } => EXIT_REFUSED,
            Self::Usage(_) => EXIT_USAGE,
            Self::Unreachable(_) => EXIT_UNREACHABLE,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::Refused {
                status,
                code,
                message,
            } if *status == 501 => write!(
                f,
                "{code}: {message} (the server does not implement this yet)"
            ),
            Self::Refused { code, message, .. } => write!(f, "{code}: {message}"),
            Self::Unreachable(message) => write!(f, "server unreachable: {message}"),
        }
    }
}

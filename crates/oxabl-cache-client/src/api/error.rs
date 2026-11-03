use std::time::Duration;

/// A non-2xx answer, as the error envelope describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusError {
    pub status: u16,
    pub code: Option<String>,
    pub reason: Option<String>,
    pub retry_after: Option<Duration>,
}

/// Why a call to the server failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApiError {
    /// No answer: connection, timeout or transfer failure.
    Transport,
    Status(StatusError),
    /// The answer is not what the protocol defines.
    Malformed,
}

impl ApiError {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Status(e) => Some(e.status),
            _ => None,
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Status(e) => e.reason.as_deref(),
            _ => None,
        }
    }

    /// Whether the same request may succeed later: no answer, 429 or a server error.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Transport => true,
            Self::Status(e) => e.status == 429 || e.status >= 500,
            Self::Malformed => false,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Status(e) => e.retry_after,
            _ => None,
        }
    }
}

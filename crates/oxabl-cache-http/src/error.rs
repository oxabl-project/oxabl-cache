//! The error envelope: every non-2xx response is JSON with a stable code.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxabl_cache_core::{Conflict, Error};
use oxabl_cache_format::FormatError;

/// An error that renders as the protocol's envelope.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) retryable: bool,
    pub(crate) retry_after: Option<u64>,
    pub(crate) reason: Option<&'static str>,
    pub(crate) detail: Option<String>,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retryable: false,
            retry_after: None,
            reason: None,
            detail: None,
        }
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub(crate) fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "not found")
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub(crate) fn too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "request too large",
        )
    }

    pub(crate) fn not_implemented(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_IMPLEMENTED, "not_configured", message)
    }

    /// The code of a status the framework answered by itself.
    pub(crate) fn for_status(status: StatusCode) -> Self {
        let (code, message) = match status {
            StatusCode::NOT_FOUND => ("not_found", "not found"),
            StatusCode::METHOD_NOT_ALLOWED => ("bad_request", "method not allowed"),
            StatusCode::PAYLOAD_TOO_LARGE => ("too_large", "request too large"),
            StatusCode::UNAUTHORIZED => ("unauthenticated", "unauthenticated"),
            s if s.is_server_error() => ("internal", "internal error"),
            _ => ("bad_request", "bad request"),
        };
        let mut error = Self::new(status, code, message);
        error.retryable = status.is_server_error();
        error
    }
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        match error {
            Error::Unauthenticated => Self::new(
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
                "unauthenticated",
            ),
            Error::Forbidden(why) => Self::forbidden(why),
            Error::TenantSuspended => Self::new(
                StatusCode::FORBIDDEN,
                "tenant_suspended",
                "the tenant is suspended",
            ),
            Error::NotFound => Self::not_found(),
            Error::Conflict(kind) => {
                let reason = match kind {
                    Conflict::Exists => "exists",
                    Conflict::PackClosed => "pack_closed",
                    Conflict::PackChanged => "pack_changed",
                    Conflict::PackIncomplete => "pack_incomplete",
                };
                let mut conflict = Self::new(StatusCode::CONFLICT, "conflict", reason);
                conflict.reason = Some(reason);
                conflict
            }
            Error::PreconditionFailed => Self::new(
                StatusCode::PRECONDITION_FAILED,
                "precondition_failed",
                "the version in If-Match is stale",
            ),
            Error::BadRequest(why) => Self::bad_request(why),
            Error::TooLarge => Self::too_large(),
            Error::QuotaExceeded => {
                Self::new(StatusCode::FORBIDDEN, "quota_exceeded", "quota exceeded")
            }
            Error::RateLimited { retry_after_secs } => {
                let mut limited = Self::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    "rate limited",
                );
                limited.retryable = true;
                limited.retry_after = Some(retry_after_secs);
                limited
            }
            Error::Unavailable(detail) => {
                let mut down = Self::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unavailable",
                    "the service is temporarily unavailable",
                );
                down.retryable = true;
                down.retry_after = Some(5);
                down.detail = Some(detail);
                down
            }
            Error::NotConfigured(what) => {
                Self::not_implemented(format!("{what} is not configured on this server"))
            }
            Error::Internal(detail) => {
                let mut internal = Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal error",
                );
                internal.retryable = true;
                internal.detail = Some(detail);
                internal
            }
        }
    }
}

impl From<FormatError> for ApiError {
    fn from(error: FormatError) -> Self {
        match error {
            FormatError::UnsupportedVersion(_) => Self::new(
                StatusCode::BAD_REQUEST,
                "unsupported_version",
                error.to_string(),
            ),
            other => Self::bad_request(other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = self.status;
        if let Some(seconds) = self.retry_after
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response.extensions_mut().insert(self);
        response
    }
}

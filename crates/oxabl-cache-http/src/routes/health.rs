//! `GET /livez` and `GET /readyz` on the metrics listener, `GET /v1/health` on the public one.

use crate::app::{App, Backend};
use crate::error::ApiError;
use crate::health::unreachable;
use crate::json::empty;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use std::sync::Arc;

/// The process is up.
pub(crate) async fn live() -> Response {
    empty(StatusCode::OK)
}

/// Metadata and store answer within the probe timeout.
pub(crate) async fn ready<B: Backend>(
    State(app): State<Arc<App<B>>>,
) -> Result<Response, ApiError> {
    let down = unreachable(&app).await;
    if down.is_empty() {
        return Ok(empty(StatusCode::OK));
    }
    let mut error = ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "unavailable",
        format!("not ready: {}", down.join(", ")),
    );
    error.retryable = true;
    error.retry_after = Some(1);
    Err(error)
}

/// The public liveness answer: no body, no detail, no dependency touched.
pub(crate) async fn public() -> Response {
    empty(StatusCode::NO_CONTENT)
}

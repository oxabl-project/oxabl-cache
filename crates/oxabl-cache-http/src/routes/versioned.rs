//! Documents under `If-Match: "v<n>"` versioning: the roster and the dictionary pointer.

use crate::error::ApiError;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;

/// The binary content type of the protocol.
pub(crate) const BINARY: &str = "application/vnd.oxabl-cache.v1";

/// The version named by `If-Match: "v<n>"`.
pub(crate) fn if_match(headers: &HeaderMap) -> Result<u64, ApiError> {
    let text = headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::bad_request("If-Match is required"))?;
    text.strip_prefix("\"v")
        .and_then(|rest| rest.strip_suffix('"'))
        .and_then(|number| number.parse().ok())
        .ok_or_else(|| ApiError::bad_request("If-Match is \"v<version>\""))
}

/// Requires the binary content type on a request body.
pub(crate) fn require_binary(headers: &HeaderMap) -> Result<(), ApiError> {
    let declared = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if declared == Some(BINARY) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "content-type must be application/vnd.oxabl-cache.v1",
        ))
    }
}

/// A binary response, optionally with an `ETag` for a version.
pub(crate) fn binary(bytes: Vec<u8>, version: Option<u64>) -> Response {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(BINARY));
    if let Some(version) = version
        && let Ok(value) = HeaderValue::from_str(&format!("\"v{version}\""))
    {
        headers.insert(header::ETAG, value);
    }
    response
}

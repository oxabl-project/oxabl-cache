//! A single `Range: bytes=…` header on a pack object.

use crate::error::ApiError;
use axum::http::{HeaderMap, StatusCode, header};

/// The inclusive byte span to serve out of an object of `size` bytes: the whole
/// object without a `Range` header, otherwise the requested span. Anything the
/// object cannot satisfy is a 416.
pub(crate) fn requested_span(headers: &HeaderMap, size: u64) -> Result<(u64, u64, bool), ApiError> {
    let unsatisfiable = || {
        ApiError::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "bad_request",
            "the range lies outside the object",
        )
    };
    let Some(value) = headers.get(header::RANGE) else {
        return Ok((0, size.saturating_sub(1), false));
    };
    let text = value.to_str().map_err(|_| unsatisfiable())?;
    let spec = text.strip_prefix("bytes=").ok_or_else(unsatisfiable)?;
    let (first, last) = spec.split_once('-').ok_or_else(unsatisfiable)?;
    let (start, end) = match (first, last) {
        ("", suffix) => {
            let length: u64 = suffix.parse().map_err(|_| unsatisfiable())?;
            if length == 0 || size == 0 {
                return Err(unsatisfiable());
            }
            (size.saturating_sub(length), size - 1)
        }
        (first, "") => (
            first.parse().map_err(|_| unsatisfiable())?,
            size.saturating_sub(1),
        ),
        (first, last) => (
            first.parse().map_err(|_| unsatisfiable())?,
            last.parse().map_err(|_| unsatisfiable())?,
        ),
    };
    if size == 0 || start > end || end >= size {
        return Err(unsatisfiable());
    }
    Ok((start, end, true))
}

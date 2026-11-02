//! Reading request bodies with a hard size limit enforced while streaming.

use crate::error::ApiError;
use axum::body::{Body, HttpBody};
use bytes::{Bytes, BytesMut};
use http_body_util::BodyExt;

/// Reads the whole body, failing with 413 as soon as it exceeds `limit` bytes.
pub(crate) async fn read_limited(body: Body, limit: usize) -> Result<Bytes, ApiError> {
    let mut body = body;
    if body.size_hint().lower() > limit as u64 {
        return Err(ApiError::too_large());
    }
    let mut collected = BytesMut::new();
    while let Some(frame) = body.frame().await {
        let frame =
            frame.map_err(|_| ApiError::bad_request("the request body could not be read"))?;
        if let Ok(data) = frame.into_data() {
            if collected.len() + data.len() > limit {
                return Err(ApiError::too_large());
            }
            collected.extend_from_slice(&data);
        }
    }
    Ok(collected.freeze())
}

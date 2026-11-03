use reqwest::Response;
use tracing::trace;

use super::error::ApiError;
use crate::progress::Progress;
use crate::stats::Counters;

/// Reads a response body of at most `cap` bytes, counting what arrives.
pub(crate) async fn read_capped(
    mut response: Response,
    cap: usize,
    progress: &Progress,
    counters: &Counters,
) -> Result<Vec<u8>, ApiError> {
    if response.content_length().is_some_and(|n| n > cap as u64) {
        return Err(ApiError::Malformed);
    }
    let mut body = Vec::with_capacity(response.content_length().map_or(0, |n| n as usize));
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                progress.touch();
                counters.received(chunk.len() as u64);
                if body.len() + chunk.len() > cap {
                    return Err(ApiError::Malformed);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(body),
            Err(e) => {
                trace!(error = %e, "response body failed");
                return Err(ApiError::Transport);
            }
        }
    }
}

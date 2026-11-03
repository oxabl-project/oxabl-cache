//! Mapping S3 answers onto the core error.

use oxabl_cache_core::Error;
use reqwest::Response;

/// A transport failure never carries a URL: presigned URLs are capabilities.
pub(crate) fn transport(error: reqwest::Error) -> Error {
    Error::unavailable(format!(
        "object store request failed: {}",
        error.without_url()
    ))
}

fn code_of(body: &str) -> Option<&str> {
    let start = body.find("<Code>")? + "<Code>".len();
    let end = body[start..].find("</Code>")?;
    Some(&body[start..start + end])
}

/// An unexpected status, with the S3 error code when the body carries one.
pub(crate) async fn unexpected(operation: &str, response: Response) -> Error {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    match code_of(&body) {
        Some(code) => {
            Error::unavailable(format!("object store refused {operation}: {status} {code}"))
        }
        None => Error::unavailable(format!("object store refused {operation}: {status}")),
    }
}

/// Passes a success through; every other status is an error.
pub(crate) async fn expect_success(operation: &str, response: Response) -> Result<Response, Error> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(unexpected(operation, response).await)
    }
}

//! Ranged reads.

use crate::error::{transport, unexpected};
use crate::signing::Window;
use crate::store::S3Store;
use oxabl_cache_core::{Conflict, Error, Result};
use reqwest::StatusCode;
use reqwest::header::{IF_MATCH, RANGE};
use rusty_s3::actions::GetObject;

impl S3Store {
    /// Reads `length` bytes at `offset` of `key`. With `if_match`, a replaced object is
    /// `Conflict(PackChanged)`; a missing one is `NotFound`; a short or oversized answer
    /// is an error.
    pub(crate) async fn read_key_range(
        &self,
        key: &str,
        offset: u64,
        length: u64,
        if_match: Option<&str>,
    ) -> Result<Vec<u8>> {
        if length == 0 {
            return Ok(Vec::new());
        }
        let last = offset
            .checked_add(length - 1)
            .ok_or_else(|| Error::bad_request("range outside the object"))?;
        let action = GetObject::new(&self.bucket, Some(&self.credentials), key);
        let url = Window::own_call().sign(&action);
        let mut request = self
            .http
            .get(url)
            .header(RANGE, format!("bytes={offset}-{last}"));
        if let Some(etag) = if_match {
            request = request.header(IF_MATCH, etag);
        }
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        match status {
            StatusCode::OK | StatusCode::PARTIAL_CONTENT => {}
            StatusCode::NOT_FOUND => return Err(Error::NotFound),
            StatusCode::PRECONDITION_FAILED => return Err(Error::Conflict(Conflict::PackChanged)),
            StatusCode::RANGE_NOT_SATISFIABLE => {
                return Err(Error::bad_request("range outside the object"));
            }
            _ => return Err(unexpected("GET", response).await),
        }
        let bytes = response.bytes().await.map_err(transport)?;
        let ignored_range = status == StatusCode::OK && offset != 0;
        if ignored_range || u64::try_from(bytes.len()).ok() != Some(length) {
            return Err(Error::unavailable(
                "object store answered a ranged GET with the wrong bytes",
            ));
        }
        Ok(bytes.to_vec())
    }
}

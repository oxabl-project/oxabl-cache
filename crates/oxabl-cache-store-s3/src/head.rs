//! Size and ETag of an object.

use crate::error::{transport, unexpected};
use crate::signing::Window;
use crate::store::S3Store;
use oxabl_cache_core::ports::ObjectHead;
use oxabl_cache_core::{Error, Result};
use reqwest::StatusCode;
use reqwest::header::{CONTENT_LENGTH, ETAG};
use rusty_s3::actions::HeadObject;

impl S3Store {
    /// `None` if the object does not exist; a store that answers without a size or an
    /// ETag is an error.
    pub(crate) async fn head_key(&self, key: &str) -> Result<Option<ObjectHead>> {
        let action = HeadObject::new(&self.bucket, Some(&self.credentials), key);
        let url = Window::own_call().sign(&action);
        let response = self.http.head(url).send().await.map_err(transport)?;
        match response.status() {
            StatusCode::OK => {
                let header = |name| {
                    response
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                };
                let size = header(CONTENT_LENGTH).and_then(|text| text.parse().ok());
                let etag = header(ETAG).filter(|etag| !etag.is_empty());
                match (size, etag) {
                    (Some(size), Some(etag)) => Ok(Some(ObjectHead {
                        size,
                        etag: etag.to_owned(),
                    })),
                    _ => Err(Error::unavailable(
                        "object store answered HEAD without a size and an ETag",
                    )),
                }
            }
            StatusCode::NOT_FOUND => Ok(None),
            _ => Err(unexpected("HEAD", response).await),
        }
    }
}

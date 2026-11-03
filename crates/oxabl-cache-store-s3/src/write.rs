//! Whole-object writes.

use crate::checksum;
use crate::error::{expect_success, transport};
use crate::signing::Window;
use crate::store::{S3Store, UPLOAD_TIMEOUT};
use oxabl_cache_core::ports::{ObjectHead, SignedUrl};
use oxabl_cache_core::{Error, Result};
use reqwest::Response;

impl S3Store {
    /// Sends `bytes` to a signed PUT URL with exactly its signed headers.
    pub(crate) async fn send_put(&self, signed: &SignedUrl, bytes: Vec<u8>) -> Result<Response> {
        let mut request = self.http.put(&signed.url).timeout(UPLOAD_TIMEOUT);
        for (name, value) in &signed.headers {
            request = request.header(name, value);
        }
        request.body(bytes).send().await.map_err(transport)
    }

    /// Writes a whole object, signing the optional guards the store supports, and checks
    /// that it is visible at the written size.
    pub(crate) async fn put_key(&self, key: &str, bytes: Vec<u8>) -> Result<ObjectHead> {
        let size = u64::try_from(bytes.len()).map_err(|_| Error::TooLarge)?;
        let digest = self
            .optional
            .checksum_sha256
            .then(|| checksum::sha256(&bytes));
        let signed = self.put_signed(
            key,
            size,
            digest.as_ref(),
            self.optional.if_none_match,
            &Window::own_call(),
        );
        let response = self.send_put(&signed, bytes).await?;
        expect_success("PUT", response).await?;
        match self.head_key(key).await? {
            Some(head) if head.size == size => Ok(head),
            _ => Err(Error::unavailable(
                "object store did not show the written object at its size",
            )),
        }
    }
}

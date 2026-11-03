//! Batch deletes.

use crate::delete_answer;
use crate::error::{expect_success, transport};
use crate::signing::Window;
use crate::store::S3Store;
use oxabl_cache_core::{Error, Result};
use rusty_s3::actions::ObjectIdentifier;

const BATCH: usize = 1000;

impl S3Store {
    /// Deletes `keys` with `DeleteObjects`. A missing key counts as deleted; any other
    /// per-key failure, or a key the answer does not account for, is an error.
    pub(crate) async fn delete_keys(&self, keys: &[String]) -> Result<()> {
        for batch in keys.chunks(BATCH) {
            let objects: Vec<ObjectIdentifier> =
                batch.iter().cloned().map(ObjectIdentifier::new).collect();
            let action = self
                .bucket
                .delete_objects(Some(&self.credentials), objects.iter());
            let url = Window::own_call().sign(&action);
            let (body, md5) = action.body_with_md5();
            let response = self
                .http
                .post(url)
                .header("content-md5", md5)
                .body(body)
                .send()
                .await
                .map_err(transport)?;
            let response = expect_success("DeleteObjects", response).await?;
            let text = response.text().await.map_err(transport)?;
            let answer = delete_answer::parse(&text)?;
            if let Some(code) = answer.error_codes.iter().find(|code| *code != "NoSuchKey") {
                return Err(Error::unavailable(format!(
                    "object store could not delete an object: {code}"
                )));
            }
            if answer.deleted + answer.error_codes.len() != batch.len() {
                return Err(Error::unavailable(
                    "object store did not account for every deleted key",
                ));
            }
        }
        Ok(())
    }
}

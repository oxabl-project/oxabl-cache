//! Listing a prefix.

use crate::error::{expect_success, transport};
use crate::layout::Layout;
use crate::signing::Window;
use crate::store::S3Store;
use jiff::Timestamp;
use oxabl_cache_core::ports::ObjectInfo;
use oxabl_cache_core::{Error, Result};
use rusty_s3::actions::ListObjectsV2;

impl S3Store {
    /// Every key and size under `prefix`, following continuation tokens to the end.
    pub(crate) async fn list_prefix(&self, prefix: &str) -> Result<Vec<(String, u64, i64)>> {
        let mut found = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut action = self.bucket.list_objects_v2(Some(&self.credentials));
            action.with_prefix(prefix);
            if let Some(token) = &token {
                action.with_continuation_token(token.as_str());
            }
            let url = Window::own_call().sign(&action);
            let response = self.http.get(url).send().await.map_err(transport)?;
            let response = expect_success("ListObjectsV2", response).await?;
            let text = response.text().await.map_err(transport)?;
            let page = ListObjectsV2::parse_response(&text).map_err(|error| {
                Error::unavailable(format!("object store ListObjectsV2 answer: {error}"))
            })?;
            let now = Timestamp::now().as_second();
            for object in page.contents {
                let modified = object
                    .last_modified
                    .as_deref()
                    .and_then(|text| text.parse::<Timestamp>().ok())
                    .map_or(now, |time| time.as_second());
                found.push((object.key, object.size.unwrap_or(0), modified));
            }
            match page.next_continuation_token {
                Some(next) => token = Some(next),
                None => return Ok(found),
            }
        }
    }

    /// The packs of one cache.
    pub(crate) async fn list_cache(
        &self,
        cache: &oxabl_cache_core::ids::CacheId,
    ) -> Result<Vec<ObjectInfo>> {
        let prefix = self.layout.cache_prefix(cache);
        Ok(self
            .list_prefix(&prefix)
            .await?
            .into_iter()
            .filter_map(|(key, size, modified_at)| {
                Layout::pack_of(&prefix, &key).map(|pack| ObjectInfo {
                    pack,
                    size,
                    modified_at,
                })
            })
            .collect())
    }
}

use oxabl_cache_format::PackId;
use tokio::sync::Mutex;

use crate::api::Api;
use crate::progress::Progress;

/// A pack a lookup response refers to, with the URL currently valid for it.
pub(crate) struct PackSlot {
    pub id: PackId,
    url: Mutex<String>,
}

impl PackSlot {
    pub fn new(id: PackId, url: String) -> Self {
        Self {
            id,
            url: Mutex::new(url),
        }
    }

    pub async fn url(&self) -> String {
        self.url.lock().await.clone()
    }

    /// Replaces `stale` with a fresh URL, unless another reader already did. Returns the URL
    /// to use now, or `None` when the server gave none.
    pub async fn renew(&self, api: &Api, stale: &str, progress: &Progress) -> Option<String> {
        let mut current = self.url.lock().await;
        if *current != stale {
            return Some(current.clone());
        }
        let mut fresh = api.urls(&[self.id], progress).await.ok()?;
        let (url, _) = fresh.remove(&self.id)?;
        current.clone_from(&url);
        Some(url)
    }
}

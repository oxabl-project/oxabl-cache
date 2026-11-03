use std::collections::HashMap;
use std::sync::Arc;

use oxabl_cache_crypto::Dictionary;
use oxabl_cache_format::SegmentId;

/// Verified dictionaries by segment id, kept for the life of the handle.
#[derive(Default)]
pub(crate) struct Dictionaries {
    map: tokio::sync::Mutex<HashMap<SegmentId, Arc<Dictionary>>>,
}

impl Dictionaries {
    pub async fn get(&self, id: &SegmentId) -> Option<Arc<Dictionary>> {
        self.map.lock().await.get(id).cloned()
    }

    pub async fn insert(&self, dictionary: Dictionary) -> Arc<Dictionary> {
        let dictionary = Arc::new(dictionary);
        self.map
            .lock()
            .await
            .insert(dictionary.id(), dictionary.clone());
        dictionary
    }
}

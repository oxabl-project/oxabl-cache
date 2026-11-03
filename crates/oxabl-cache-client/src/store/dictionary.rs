use std::sync::Arc;

use oxabl_cache_crypto::{Dictionary, open_dictionary, verify_pointer};

use crate::cache::{Inner, RosterView};
use crate::fetch::{PackSlot, read_range};
use crate::progress::Progress;

/// The cache's published dictionary, if its pointer verifies against the current roster and
/// its segment verifies against the pointer's id. Otherwise the cache has none for writers.
pub(crate) async fn writer_dictionary(inner: &Inner, view: &RosterView) -> Option<Arc<Dictionary>> {
    let progress = Progress::new();
    let document = inner.api.dictionary(&progress).await.ok()??;
    let pointer = verify_pointer(&document.pointer, &view.roster).ok()?;
    let id = pointer.dict_segment_id;
    if let Some(known) = inner.dictionaries.get(&id).await {
        return Some(known);
    }
    let slot = PackSlot::new(document.pack, document.url);
    let _connection = inner.gates.connections.acquire().await;
    let bytes = read_range(
        &inner.api,
        &slot,
        document.offset,
        document.offset + u64::from(document.length),
        &progress,
    )
    .await
    .ok()?;
    let segment = view.segment.clone();
    let level = inner.zstd_level;
    let opened = tokio::task::spawn_blocking(move || open_dictionary(&segment, &bytes, &id, level))
        .await
        .ok()?
        .ok()?;
    Some(inner.dictionaries.insert(opened).await)
}

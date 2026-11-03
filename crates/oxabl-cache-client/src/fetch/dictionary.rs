use std::sync::Arc;

use oxabl_cache_crypto::{Dictionary, Error, open_dictionary};
use oxabl_cache_format::SegmentId;

use super::lookup::Lookup;
use super::read::read_range;
use super::report;
use super::session::Session;
use super::types::MissReason;
use crate::cache::{RosterView, permits_for};

/// The verified dictionary `id`, from this handle's memory or from the pack the lookup
/// located it in. A dictionary the response does not list is `NotFound`; one that fails
/// verification is `Corrupt`.
pub(crate) async fn resolve(
    session: &Session,
    view: &Arc<RosterView>,
    lookup: &Lookup,
    id: SegmentId,
) -> Result<Arc<Dictionary>, MissReason> {
    let inner = &session.inner;
    if let Some(dictionary) = inner.dictionaries.get(&id).await {
        return Ok(dictionary);
    }
    let entry = lookup
        .dictionaries
        .iter()
        .find(|d| d.dict_id == id)
        .ok_or(MissReason::NotFound)?;
    let slot = &lookup.packs[entry.pack_idx as usize];
    let _budget = inner
        .gates
        .fetch_bytes
        .acquire_many(permits_for(u64::from(entry.length)))
        .await;
    let bytes = {
        let _connection = inner.gates.connections.acquire().await;
        read_range(
            &inner.api,
            slot,
            entry.offset,
            entry.offset + u64::from(entry.length),
            &session.progress,
        )
        .await?
    };
    let (view, level) = (view.clone(), inner.zstd_level);
    let opened =
        tokio::task::spawn_blocking(move || open_dictionary(&view.segment, &bytes, &id, level))
            .await
            .map_err(|_| MissReason::Corrupt)?;
    match opened {
        Ok(dictionary) => Ok(inner.dictionaries.insert(dictionary).await),
        Err(Error::SegmentId) => {
            report::hash_mismatch(inner, slot.id, id);
            Err(MissReason::Corrupt)
        }
        Err(_) => Err(MissReason::Corrupt),
    }
}

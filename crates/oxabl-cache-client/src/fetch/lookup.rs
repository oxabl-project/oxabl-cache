use std::sync::Arc;

use oxabl_cache_format::{
    LookupFlags, LookupId, LookupRequest, LookupResponse, Namespace, ResponseDictionary,
};

use super::pack_slot::PackSlot;
use super::session::Session;
use super::types::MissReason;
use crate::api::ApiError;
use crate::cache::RosterView;

/// One hit, copied out of the response so that work on it can leave the response's buffer.
pub(crate) struct RawHit {
    /// Position within the chunk.
    pub local: usize,
    /// Position among the requests of the fetch.
    pub global: usize,
    pub ns: Namespace,
    pub pack_idx: usize,
    pub offset: u64,
    pub length: u32,
    pub record: Vec<u8>,
}

/// The decoded answer to one chunk's lookup.
pub(crate) struct Lookup {
    pub ids: Arc<Vec<LookupId>>,
    pub packs: Vec<Arc<PackSlot>>,
    pub hits: Vec<RawHit>,
    pub dictionaries: Vec<ResponseDictionary>,
}

/// Looks up the requests at `indexes` in fetch mode with URLs.
pub(crate) async fn lookup(
    session: &Session,
    view: &RosterView,
    indexes: &[usize],
) -> Result<Lookup, MissReason> {
    let ids: Vec<LookupId> = indexes
        .iter()
        .map(|&i| view.lookup.lookup_id(&session.keys[i].0))
        .collect();
    let request = LookupRequest {
        flags: LookupFlags {
            records: true,
            urls: true,
        },
        namespaces: session.inner.namespaces,
        ids,
    };
    let shape = request.shape().map_err(|_| MissReason::Corrupt)?;
    let frame = request.encode().map_err(|_| MissReason::Corrupt)?;
    let bytes = session
        .inner
        .api
        .lookup(frame, &session.progress)
        .await
        .map_err(|e| match e {
            ApiError::Malformed => MissReason::Corrupt,
            _ => MissReason::Unavailable,
        })?;
    let response = LookupResponse::decode(&bytes, &shape).map_err(|_| MissReason::Corrupt)?;
    let packs = response
        .packs
        .iter()
        .map(|p| Arc::new(PackSlot::new(p.pack_id, p.url.to_owned())))
        .collect();
    let mut hits = Vec::with_capacity(response.hits.len());
    for hit in &response.hits {
        let Some(located) = &hit.record else {
            return Err(MissReason::Corrupt);
        };
        hits.push(RawHit {
            local: hit.index as usize,
            global: indexes[hit.index as usize],
            ns: hit.ns,
            pack_idx: located.pack_idx as usize,
            offset: located.offset,
            length: located.length,
            record: located.record.to_vec(),
        });
    }
    Ok(Lookup {
        ids: Arc::new(request.ids),
        packs,
        hits,
        dictionaries: response.dictionaries,
    })
}

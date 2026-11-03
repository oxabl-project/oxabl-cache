use oxabl_cache_format::limits::LOOKUP_IDS;
use oxabl_cache_format::{LookupFlags, LookupId, LookupRequest};
use tokio::task::JoinSet;

use super::Inner;
use crate::compile_key::CompileKey;
use crate::error::CacheError;
use crate::progress::Progress;

/// Whether the cache holds an entry for a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Hit,
    Miss,
}

const CONCURRENT_LOOKUPS: usize = 4;

pub(super) async fn probe(inner: &Inner, keys: &[CompileKey]) -> Result<Vec<Presence>, CacheError> {
    let view = inner.view();
    let ids: Vec<LookupId> = keys.iter().map(|k| view.lookup.lookup_id(&k.0)).collect();
    let mut presence = vec![Presence::Miss; keys.len()];
    let mut pending = ids.chunks(LOOKUP_IDS).enumerate();
    let mut running = JoinSet::new();
    loop {
        while running.len() < CONCURRENT_LOOKUPS {
            let Some((chunk, ids)) = pending.next() else {
                break;
            };
            let request = LookupRequest {
                flags: LookupFlags {
                    records: false,
                    urls: false,
                },
                namespaces: inner.namespaces,
                ids: ids.to_vec(),
            };
            let api = inner.api.clone();
            running.spawn(async move {
                let shape = request.shape().map_err(|_| CacheError::Unavailable)?;
                let frame = request.encode().map_err(|_| CacheError::Unavailable)?;
                let bytes = api
                    .lookup(frame, &Progress::new())
                    .await
                    .map_err(|_| CacheError::Unavailable)?;
                let response = oxabl_cache_format::LookupResponse::decode(&bytes, &shape)
                    .map_err(|_| CacheError::Unavailable)?;
                let hits: Vec<u32> = response.hits.iter().map(|h| h.index).collect();
                Ok::<_, CacheError>((chunk, hits))
            });
        }
        let Some(done) = running.join_next().await else {
            break;
        };
        let (chunk, hits) = done.map_err(|_| CacheError::Unavailable)??;
        for index in hits {
            presence[chunk * LOOKUP_IDS + index as usize] = Presence::Hit;
        }
    }
    Ok(presence)
}

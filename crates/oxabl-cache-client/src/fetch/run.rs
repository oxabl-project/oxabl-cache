use std::sync::Arc;

use oxabl_cache_format::limits::LOOKUP_IDS;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::chunk::process_chunk;
use super::session::Session;
use super::types::MissReason;
use crate::cache::RosterView;
use crate::roster_refresh::refresh;

const LOOKUPS_IN_FLIGHT: usize = 2;

/// Runs every chunk of lookups. Requests that need a newer roster wait for one refetch of it
/// and run once more; whatever is still ahead after that is a miss.
pub(crate) async fn run(session: Arc<Session>) {
    let view = session.inner.view();
    let all: Vec<usize> = (0..session.keys.len()).collect();
    let gate = Arc::new(Semaphore::new(LOOKUPS_IN_FLIGHT));
    let mut chunks = JoinSet::new();
    for chunk in all.chunks(LOOKUP_IDS) {
        let (session, view, gate, chunk) =
            (session.clone(), view.clone(), gate.clone(), chunk.to_vec());
        chunks.spawn(async move {
            let _turn = gate.acquire().await;
            process_chunk(&session, &view, &chunk, true).await
        });
    }
    let mut ahead = Vec::new();
    while let Some(done) = chunks.join_next().await {
        ahead.extend(done.unwrap_or_default());
    }
    if ahead.is_empty() {
        return;
    }
    let newer: Option<Arc<RosterView>> = session
        .refreshed
        .get_or_init(|| refresh(session.inner.clone()))
        .await
        .clone();
    match newer {
        Some(view) => {
            for chunk in ahead.chunks(LOOKUP_IDS) {
                process_chunk(&session, &view, chunk, false).await;
            }
        }
        None => {
            for index in ahead {
                session.miss(index, MissReason::Untrusted).await;
            }
        }
    }
}

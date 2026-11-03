//! Reachability of the metadata database and the object store, within a bound.

use crate::app::{App, Backend};
use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::{ObjectKey, ObjectStore};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// The dependencies that did not answer within the timeout; empty when ready.
pub(crate) async fn unreachable<B: Backend>(app: &App<B>) -> Vec<&'static str> {
    let metadata = async {
        tokio::time::timeout(PROBE_TIMEOUT, app.engine.live_node_count())
            .await
            .is_ok_and(|answer| answer.is_ok())
    };
    let store = async {
        let probe = ObjectKey {
            cache: CacheId::random(),
            pack: PackId::random(),
        };
        tokio::time::timeout(PROBE_TIMEOUT, app.engine.object_store().head(&probe))
            .await
            .is_ok_and(|answer| answer.is_ok())
    };
    let (metadata, store) = tokio::join!(metadata, store);
    [(metadata, "metadata"), (store, "store")]
        .into_iter()
        .filter(|(reachable, _)| !reachable)
        .map(|(_, name)| name)
        .collect()
}

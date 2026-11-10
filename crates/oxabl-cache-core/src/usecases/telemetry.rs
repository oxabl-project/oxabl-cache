//! Sealed telemetry: members append opaque reports and read them back to aggregate.

use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::domain::telemetry::StoredTelemetry;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::CacheId;
use crate::limits::{TELEMETRY_KEEP, TELEMETRY_PAGE, TELEMETRY_SECS};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler, TxScope};
use crate::usecases::scoped_cache::scoped_cache;
use oxabl_cache_format::SealedTelemetry;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Stores a sealed report whose outer shape is an `OXTL` frame. Needs `read`. The cache
    /// keeps its newest reports within a count and an age.
    pub async fn submit_telemetry(
        &self,
        caller: &Caller,
        cache: CacheId,
        body: Vec<u8>,
    ) -> Result<u64> {
        SealedTelemetry::decode(&body)
            .map_err(|e| Error::bad_request(format!("not a telemetry report: {e}")))?;
        let caller = caller.clone();
        let now = self.now();
        self.meta
            .transact_in(TxScope::Cache(cache), move |tx| {
                scoped_cache(tx, &caller, &cache, Scope::Read)?;
                tx.append_telemetry(&cache, now, &body, TELEMETRY_KEEP, now - TELEMETRY_SECS)
            })
            .await
    }

    /// Up to `limit` (at most one page) reports after `after`, oldest first. Needs `read`.
    pub async fn telemetry(
        &self,
        caller: &Caller,
        cache: CacheId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredTelemetry>> {
        if limit == 0 || limit > TELEMETRY_PAGE {
            return Err(Error::bad_request(format!(
                "limit is 1 to {TELEMETRY_PAGE}"
            )));
        }
        let caller = caller.clone();
        let oldest = self.now() - TELEMETRY_SECS;
        self.meta
            .read(move |tx| {
                scoped_cache(tx, &caller, &cache, Scope::Read)?;
                let rows = tx.telemetry_after(&cache, after, limit)?;
                Ok(rows
                    .into_iter()
                    .filter(|row| row.received_at >= oldest)
                    .collect())
            })
            .await
    }
}

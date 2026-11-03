//! Client reports of corrupt segments, verified by the server before it acts.

use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{CacheId, PackId, SegmentId};
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler};
use crate::usecases::scoped_cache::scoped_cache;

/// What a report led to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportOutcome {
    /// The segment is corrupt; the whole pack was verified and corrupt entries removed.
    Confirmed {
        /// Entries deleted.
        entries_removed: u64,
    },
    /// The server's own check did not confirm the report; nothing changed.
    Unconfirmed,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Verifies a reported segment by hashing its stored bytes. Only a mismatch
    /// (or a missing object) acts. Needs `read`; at most the configured number of
    /// verifications per principal per window.
    pub async fn report_corruption(
        &self,
        caller: &Caller,
        cache: CacheId,
        pack: PackId,
        segment: SegmentId,
    ) -> Result<ReportOutcome> {
        let authorized = caller.clone();
        self.meta
            .read(move |tx| {
                scoped_cache(tx, &authorized, &cache, Scope::Read)?;
                Ok(())
            })
            .await?;
        self.reports
            .admit(
                caller.principal,
                self.now(),
                self.config.report_limit,
                self.config.report_window_secs,
            )
            .map_err(|retry_after_secs| Error::RateLimited { retry_after_secs })?;
        let reported = self
            .meta
            .read(move |tx| tx.segment(&cache, &segment))
            .await?;
        let Some(found) = reported.filter(|s| s.pack == pack && s.dead_since.is_none()) else {
            return Ok(ReportOutcome::Unconfirmed);
        };
        if self
            .segment_intact(&ObjectKey { cache, pack }, &found)
            .await?
        {
            return Ok(ReportOutcome::Unconfirmed);
        }
        let outcome = self.verify_pack(cache, pack).await?;
        Ok(ReportOutcome::Confirmed {
            entries_removed: outcome.entries_removed,
        })
    }
}

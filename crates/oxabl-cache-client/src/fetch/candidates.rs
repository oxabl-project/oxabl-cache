use std::sync::Arc;

use oxabl_cache_crypto::{AcceptedRecord, Error, RecordContext, open_record};
use oxabl_cache_format::{LookupId, Namespace};
use tokio::task::JoinSet;

use super::lookup::{Lookup, RawHit};
use super::session::Session;
use super::types::MissReason;
use crate::artifact_path::ArtifactPath;
use crate::cache::RosterView;

/// A hit whose record verified and whose signer the roster accepts for its namespace.
pub(crate) struct Candidate {
    pub local: usize,
    pub global: usize,
    pub ns: Namespace,
    pub accepted: AcceptedRecord,
    pub files: Vec<(ArtifactPath, u64)>,
    pub pack_idx: usize,
    pub offset: u64,
    pub length: u32,
}

/// Why a hit is not a candidate.
pub(crate) enum Rejection {
    Miss(MissReason),
    /// The record names a roster version newer than the one held.
    Ahead,
}

pub(crate) struct Opened {
    pub local: usize,
    pub result: Result<Candidate, Rejection>,
}

const BATCH: usize = 128;

fn open_one(
    session: &Session,
    view: &RosterView,
    ids: &[LookupId],
    hit: RawHit,
) -> Result<Candidate, Rejection> {
    let inner = &session.inner;
    let context = RecordContext {
        cache_id: inner.pins.cache,
        epoch: view.epoch(),
        namespace: hit.ns,
        lookup_id: ids[hit.local],
    };
    let corrupt = |_| Rejection::Miss(MissReason::Corrupt);
    let opened =
        open_record(&view.record, &context, &hit.record, &inner.limits).map_err(corrupt)?;
    let accepted = opened
        .accept(&view.roster, &inner.reader_key)
        .map_err(|e| match e {
            Error::SignerNotAccepted => Rejection::Miss(MissReason::Untrusted),
            Error::RosterVersionAhead => Rejection::Ahead,
            _ => Rejection::Miss(MissReason::Corrupt),
        })?;
    let fields = &accepted.entry().fields;
    let files = fields
        .files
        .iter()
        .map(|f| ArtifactPath::new(&f.path).map(|p| (p, f.size)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Rejection::Miss(MissReason::Corrupt))?;
    if let Some(expected) = &session.expects[hit.global] {
        let same = expected.len() == files.len()
            && expected.iter().all(|p| files.iter().any(|(q, _)| q == p));
        if !same {
            return Err(Rejection::Miss(MissReason::PathMismatch));
        }
    }
    Ok(Candidate {
        local: hit.local,
        global: hit.global,
        ns: hit.ns,
        accepted,
        files,
        pack_idx: hit.pack_idx,
        offset: hit.offset,
        length: hit.length,
    })
}

/// Opens every hit of a lookup on the CPU workers.
pub(crate) async fn open_all(
    session: &Arc<Session>,
    view: &Arc<RosterView>,
    lookup: &mut Lookup,
) -> Vec<Opened> {
    let mut hits = std::mem::take(&mut lookup.hits);
    let mut tasks = JoinSet::new();
    while !hits.is_empty() {
        let batch: Vec<RawHit> = hits.drain(..BATCH.min(hits.len())).collect();
        let permit = session.inner.gates.workers.clone().acquire_owned().await;
        let (session, view, ids) = (session.clone(), view.clone(), lookup.ids.clone());
        tasks.spawn_blocking(move || {
            let _permit = permit;
            batch
                .into_iter()
                .map(|hit| {
                    let local = hit.local;
                    Opened {
                        local,
                        result: open_one(&session, &view, &ids, hit),
                    }
                })
                .collect::<Vec<_>>()
        });
    }
    let mut opened = Vec::new();
    while let Some(done) = tasks.join_next().await {
        opened.extend(done.unwrap_or_default());
    }
    opened
}

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use oxabl_cache_crypto::Dictionary;
use oxabl_cache_format::{PackId, SegmentId};
use tokio::task::JoinSet;

use super::candidates::Candidate;
use super::committer::Pending;
use super::dictionary;
use super::install::{Failure, Installed, Ready, Settings, decode_install};
use super::lookup::Lookup;
use super::ranges::{Range, Wanted, coalesce};
use super::read::RangeStream;
use super::report;
use super::session::Session;
use super::types::{FetchOutcome, MissReason};
use crate::cache::{RosterView, permits_for};
use crate::install::Stage;

/// A request that failed to install from the candidate tried, by position within the chunk.
pub(crate) type Failed = (usize, MissReason);

async fn settle_installed(session: &Session, ready: &Ready, files: Vec<super::InstalledFile>) {
    let outcome = FetchOutcome::Installed {
        key: session.keys[ready.candidate.global],
        files,
        provenance: ready.candidate.accepted.provenance().into(),
    };
    session
        .sink
        .settle(ready.candidate.global, outcome, &session.progress)
        .await;
}

/// Opens one segment on a CPU worker and delivers the outcome; a failure comes back.
async fn decode_member(
    session: Arc<Session>,
    view: Arc<RosterView>,
    stage: Arc<Stage>,
    ready: Arc<Vec<Ready>>,
    item: usize,
    segment: Bytes,
    pack: PackId,
) -> Option<Failed> {
    let inner = &session.inner;
    let permit = inner.gates.workers.clone().acquire_owned().await;
    let (limits, durability, cancel) = (inner.limits, session.durability, session.cancel.clone());
    let work = {
        let ready = ready.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let settings = Settings {
                view: &view,
                limits: &limits,
                stage: &stage,
                durability,
                cancel: &cancel,
            };
            decode_install(&settings, &ready[item], &segment)
        })
    };
    let entry = &ready[item];
    match work.await {
        Ok(Ok(Installed::Done(files))) => {
            settle_installed(&session, entry, files).await;
            None
        }
        Ok(Ok(Installed::Staged(set))) => {
            let pending = Pending {
                global: entry.candidate.global,
                provenance: entry.candidate.accepted.provenance().into(),
                set,
            };
            session.committer.submit(&session, pending).await;
            None
        }
        Ok(Err(Failure::HashMismatch)) => {
            let segment = entry.candidate.accepted.entry().fields.segment_id;
            report::hash_mismatch(inner, pack, segment);
            Some((entry.candidate.local, MissReason::Corrupt))
        }
        Ok(Err(Failure::Miss(reason))) => Some((entry.candidate.local, reason)),
        Err(_) => Some((entry.candidate.local, MissReason::Corrupt)),
    }
}

/// Reads one range and hands each segment to a worker as soon as its last byte has arrived,
/// so that outcomes stream while the rest of the range is still coming.
async fn serve_range(
    session: Arc<Session>,
    view: Arc<RosterView>,
    lookup: Arc<Lookup>,
    stage: Arc<Stage>,
    ready: Arc<Vec<Ready>>,
    range: Range,
) -> Vec<Failed> {
    let inner = &session.inner;
    let _budget = inner
        .gates
        .fetch_bytes
        .clone()
        .acquire_many_owned(permits_for(range.len()))
        .await;
    let slot = &lookup.packs[range.pack_idx];
    let mut failed: Vec<Failed> = Vec::new();
    let mut decodes = JoinSet::new();
    let mut pending: VecDeque<&Wanted> = range.members.iter().collect();
    let mut buffer = BytesMut::with_capacity(range.len() as usize);
    let mut consumed = 0u64;
    let mut error = None;
    {
        let _connection = inner.gates.connections.acquire().await;
        let mut stream =
            RangeStream::new(&inner.api, slot, range.start, range.end, &session.progress);
        loop {
            match stream.next().await {
                Ok(Some(chunk)) => buffer.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(reason) => {
                    error = Some(reason);
                    break;
                }
            }
            let received = consumed + buffer.len() as u64;
            while let Some(member) = pending.front() {
                let begin = member.offset - range.start;
                let end = begin + u64::from(member.length);
                if end > received {
                    break;
                }
                let member = pending.pop_front().expect("a pending member");
                if begin < consumed {
                    failed.push((ready[member.item].candidate.local, MissReason::Corrupt));
                    continue;
                }
                let part = buffer.split_to((end - consumed) as usize).freeze();
                let segment = part.slice((begin - consumed) as usize..);
                consumed = end;
                decodes.spawn(decode_member(
                    session.clone(),
                    view.clone(),
                    stage.clone(),
                    ready.clone(),
                    member.item,
                    segment,
                    slot.id,
                ));
            }
        }
    }
    let reason = error.unwrap_or(MissReason::Corrupt);
    failed.extend(
        pending
            .iter()
            .map(|m| (ready[m.item].candidate.local, reason)),
    );
    while let Some(done) = decodes.join_next().await {
        failed.extend(done.ok().flatten());
    }
    failed
}

/// Fetches and installs one candidate per request. Successes are delivered as they finish;
/// the failures come back for the caller to try the next candidate or to settle as misses.
pub(crate) async fn execute(
    session: &Arc<Session>,
    view: &Arc<RosterView>,
    lookup: &Arc<Lookup>,
    candidates: Vec<Candidate>,
) -> Vec<Failed> {
    let stage = match &session.stage {
        Ok(stage) => stage.clone(),
        Err(kind) => {
            return candidates
                .iter()
                .map(|c| (c.local, MissReason::Install(*kind)))
                .collect();
        }
    };
    let mut failed = Vec::new();
    let mut dictionaries: HashMap<SegmentId, Result<Arc<Dictionary>, MissReason>> = HashMap::new();
    let mut ready = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let Some(id) = candidate.accepted.entry().fields.dict_id else {
            ready.push(Ready {
                candidate,
                dictionary: None,
            });
            continue;
        };
        let resolved = match dictionaries.get(&id) {
            Some(known) => known.clone(),
            None => {
                let resolved = dictionary::resolve(session, view, lookup, id).await;
                dictionaries.insert(id, resolved.clone());
                resolved
            }
        };
        match resolved {
            Ok(dictionary) => ready.push(Ready {
                candidate,
                dictionary: Some(dictionary),
            }),
            Err(reason) => failed.push((candidate.local, reason)),
        }
    }
    let wanted: Vec<Wanted> = ready
        .iter()
        .enumerate()
        .map(|(item, r)| Wanted {
            pack_idx: r.candidate.pack_idx,
            item,
            offset: r.candidate.offset,
            length: r.candidate.length,
        })
        .collect();
    let ready = Arc::new(ready);
    let mut tasks = JoinSet::new();
    for range in coalesce(wanted) {
        tasks.spawn(serve_range(
            session.clone(),
            view.clone(),
            lookup.clone(),
            stage.clone(),
            ready.clone(),
            range,
        ));
    }
    while let Some(done) = tasks.join_next().await {
        failed.extend(done.unwrap_or_default());
    }
    failed
}

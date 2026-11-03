use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use super::batch::execute;
use super::candidates::{Candidate, Opened, Rejection, open_all};
use super::lookup::lookup;
use super::session::Session;
use super::types::MissReason;
use crate::cache::RosterView;

/// What is left to try for one request.
struct Attempt {
    local: usize,
    global: usize,
    queue: VecDeque<Candidate>,
    first_failure: Option<MissReason>,
}

/// Requests whose records name a roster version newer than the one the chunk ran under.
pub(crate) type Ahead = Vec<usize>;

fn order(namespace: oxabl_cache_format::Namespace) -> u8 {
    namespace.to_u8()
}

/// Looks up the requests at `indexes`, verifies their records and installs what verifies.
/// With `allow_ahead`, requests that need a newer roster are handed back unsettled.
pub(crate) async fn process_chunk(
    session: &Arc<Session>,
    view: &Arc<RosterView>,
    indexes: &[usize],
    allow_ahead: bool,
) -> Ahead {
    let ahead = run_chunk(session, view, indexes, allow_ahead).await;
    session.committer.flush(session).await;
    let deferred: HashSet<usize> = ahead.iter().copied().collect();
    for &index in indexes.iter().filter(|i| !deferred.contains(i)) {
        session.miss(index, MissReason::Unavailable).await;
    }
    ahead
}

async fn run_chunk(
    session: &Arc<Session>,
    view: &Arc<RosterView>,
    indexes: &[usize],
    allow_ahead: bool,
) -> Ahead {
    let mut found = match lookup(session, view, indexes).await {
        Ok(found) => found,
        Err(reason) => {
            for &index in indexes {
                session.miss(index, reason).await;
            }
            return Vec::new();
        }
    };
    let mut opened = open_all(session, view, &mut found).await;
    opened.sort_by_key(|o| {
        let ns = match &o.result {
            Ok(c) => order(c.ns),
            Err(_) => u8::MAX,
        };
        (o.local, ns)
    });
    let found = Arc::new(found);

    let mut by_request: Vec<Vec<Opened>> = (0..indexes.len()).map(|_| Vec::new()).collect();
    for entry in opened {
        by_request[entry.local].push(entry);
    }
    let mut ahead = Vec::new();
    let mut active: Vec<Attempt> = Vec::new();
    for (local, entries) in by_request.into_iter().enumerate() {
        let global = indexes[local];
        if entries.is_empty() {
            session.miss(global, MissReason::NotFound).await;
            continue;
        }
        if allow_ahead
            && entries
                .iter()
                .any(|e| matches!(e.result, Err(Rejection::Ahead)))
        {
            ahead.push(global);
            continue;
        }
        let mut attempt = Attempt {
            local,
            global,
            queue: VecDeque::new(),
            first_failure: None,
        };
        for entry in entries {
            match entry.result {
                Ok(candidate) => attempt.queue.push_back(candidate),
                Err(Rejection::Miss(reason)) => {
                    attempt.first_failure.get_or_insert(reason);
                }
                Err(Rejection::Ahead) => {
                    attempt.first_failure.get_or_insert(MissReason::Untrusted);
                }
            }
        }
        if attempt.queue.is_empty() {
            let reason = attempt.first_failure.unwrap_or(MissReason::Corrupt);
            session.miss(global, reason).await;
        } else {
            active.push(attempt);
        }
    }

    while !active.is_empty() {
        let batch: Vec<Candidate> = active
            .iter_mut()
            .filter_map(|a| a.queue.pop_front())
            .collect();
        let failed: HashMap<usize, MissReason> = execute(session, view, &found, batch)
            .await
            .into_iter()
            .collect();
        let mut next = Vec::new();
        for mut attempt in active {
            let Some(&reason) = failed.get(&attempt.local) else {
                continue;
            };
            if matches!(reason, MissReason::Install(_)) {
                session.miss(attempt.global, reason).await;
            } else if attempt.queue.is_empty() {
                let reason = attempt.first_failure.unwrap_or(reason);
                session.miss(attempt.global, reason).await;
            } else {
                attempt.first_failure.get_or_insert(reason);
                next.push(attempt);
            }
        }
        active = next;
    }
    ahead
}

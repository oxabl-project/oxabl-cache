//! Fetching: batched lookups, verified ranged reads and atomic installation.

mod batch;
mod candidates;
mod chunk;
mod committer;
mod dictionary;
mod install;
mod lookup;
mod pack_slot;
mod ranges;
mod read;
mod report;
mod run;
mod session;
mod types;
mod watchdog;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures_core::Stream;
use tokio::sync::{OnceCell, mpsc};
use tokio::task::JoinHandle;

use oxabl_cache_format::LookupId;

use crate::activity::{Activity, ActivityKind};
use crate::artifact_path::ArtifactPath;
use crate::cache::Inner;
use crate::compile_key::CompileKey;
use crate::install::Stage;
use crate::progress::Progress;

pub use types::{
    FetchOptions, FetchOutcome, FetchRequest, InstalledFile, MissReason, Provenance, Tier,
};

pub(crate) use pack_slot::PackSlot;
pub(crate) use read::read_range;

use session::{Session, Sink};

const OUTCOME_BUFFER: usize = 1024;

/// Ends the work of a fetch when the stream is dropped.
struct Guard {
    task: JoinHandle<()>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.task.abort();
    }
}

/// `futures_core::Stream<Item = FetchOutcome>`. Dropping it cancels what has not completed.
pub struct Fetch {
    outcomes: mpsc::Receiver<FetchOutcome>,
    _guard: Guard,
}

impl Fetch {
    pub(crate) fn start(
        inner: Arc<Inner>,
        requests: Vec<FetchRequest>,
        options: FetchOptions,
    ) -> Fetch {
        let (keys, expects) = requests.into_iter().map(|r| (r.key, r.expect)).unzip();
        Self::launch(inner, keys, expects, None, options)
    }

    /// Fetches entries named by lookup id; each outcome's `key` holds the id.
    pub(crate) fn start_sampled(
        inner: Arc<Inner>,
        ids: Vec<LookupId>,
        options: FetchOptions,
    ) -> Fetch {
        let keys = ids.iter().map(|id| CompileKey(id.0)).collect();
        let expects = ids.iter().map(|_| None).collect();
        Self::launch(inner, keys, expects, Some(ids), options)
    }

    fn launch(
        inner: Arc<Inner>,
        keys: Vec<CompileKey>,
        expects: Vec<Option<Vec<ArtifactPath>>>,
        ids: Option<Vec<LookupId>>,
        options: FetchOptions,
    ) -> Fetch {
        let (tx, outcomes) = mpsc::channel(OUTCOME_BUFFER);
        let cancel = Arc::new(AtomicBool::new(false));
        let count = keys.len();
        let session = Arc::new(Session {
            stage: Stage::open(&options.stage)
                .map(Arc::new)
                .map_err(|e| e.kind()),
            inner: inner.clone(),
            durability: options.durability,
            idle_timeout: options.idle_timeout,
            keys,
            ids,
            expects,
            progress: Progress::new(),
            cancel: cancel.clone(),
            sink: Sink::new(tx, count),
            committer: Default::default(),
            refreshed: OnceCell::new(),
            started: std::time::Instant::now(),
        });
        let task = inner.handle.spawn(drive(session));
        Fetch {
            outcomes,
            _guard: Guard { task, cancel },
        }
    }

    /// The next outcome, in completion order; `None` once every request has one.
    pub async fn next(&mut self) -> Option<FetchOutcome> {
        self.outcomes.recv().await
    }
}

impl Stream for Fetch {
    type Item = FetchOutcome;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<FetchOutcome>> {
        self.outcomes.poll_recv(cx)
    }
}

async fn drive(session: Arc<Session>) {
    let idle = session.idle_timeout;
    tokio::select! {
        () = run::run(session.clone()) => {}
        () = watchdog::idle(&session.progress, idle) => {
            session.cancel.store(true, Ordering::Relaxed);
            session.sink.time_out(&session.keys).await;
        }
    }
    session.sink.log_failures(&session.inner.errors);
    if session.ids.is_none() && !session.keys.is_empty() {
        record_pull(&session);
    }
}

fn record_pull(session: &Session) {
    let settled = session.sink.settled();
    let count = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
    crate::activity::record(
        &session.inner,
        Activity {
            at: crate::activity::now_secs(),
            kind: ActivityKind::Pull,
            mode: session.inner.mode,
            requested: count(session.keys.len() as u64),
            hits: count(settled.installed),
            misses: count(settled.not_found),
            failed: count(settled.failed),
            bytes: settled.bytes,
            duration_ms: count(session.started.elapsed().as_millis() as u64),
            rtt_ms: 0,
        },
    );
}

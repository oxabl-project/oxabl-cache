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

use crate::cache::Inner;
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
        let (tx, outcomes) = mpsc::channel(OUTCOME_BUFFER);
        let cancel = Arc::new(AtomicBool::new(false));
        let count = requests.len();
        let (keys, expects) = requests.into_iter().map(|r| (r.key, r.expect)).unzip();
        let session = Arc::new(Session {
            stage: Stage::open(&options.stage)
                .map(Arc::new)
                .map_err(|e| e.kind()),
            inner: inner.clone(),
            durability: options.durability,
            idle_timeout: options.idle_timeout,
            keys,
            expects,
            progress: Progress::new(),
            cancel: cancel.clone(),
            sink: Sink::new(tx, count),
            committer: Default::default(),
            refreshed: OnceCell::new(),
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
}

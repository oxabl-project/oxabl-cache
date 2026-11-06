use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use oxabl_cache_format::LookupId;
use tokio::sync::{OnceCell, mpsc};

use super::committer::Committer;
use super::types::{FetchOutcome, MissReason};
use crate::artifact_path::ArtifactPath;
use crate::cache::{Inner, RosterView};
use crate::compile_key::CompileKey;
use crate::error_log::{ErrorLog, Tally, fetch_failure};
use crate::install::{Durability, Stage};
use crate::progress::Progress;

/// Delivers outcomes to the stream and remembers which requests have one.
pub(crate) struct Sink {
    tx: mpsc::Sender<FetchOutcome>,
    delivered: Mutex<Vec<bool>>,
    failures: Mutex<Tally>,
}

impl Sink {
    pub fn new(tx: mpsc::Sender<FetchOutcome>, requests: usize) -> Self {
        Self {
            tx,
            delivered: Mutex::new(vec![false; requests]),
            failures: Mutex::default(),
        }
    }

    fn count_failure(&self, reason: MissReason) {
        if let Some((kind, meaning)) = fetch_failure(reason) {
            self.failures.lock().expect("sink lock").add(kind, meaning);
        }
    }

    /// Logs one line per kind of failure the fetch settled with.
    pub fn log_failures(&self, log: &ErrorLog) {
        let requests = self.delivered.lock().expect("sink lock").len();
        let failures = self.failures.lock().expect("sink lock");
        if !failures.is_empty() {
            failures.record(log, "fetch", requests);
        }
    }

    /// Delivers the outcome of request `index`, once; later outcomes for it are dropped.
    /// The request counts as settled only when its outcome is in the stream. Waiting for room
    /// in the stream counts as progress.
    pub async fn settle(&self, index: usize, outcome: FetchOutcome, progress: &Progress) {
        if self.delivered.lock().expect("sink lock")[index] {
            return;
        }
        let permit = {
            let _waiting = progress.waiting();
            self.tx.reserve().await
        };
        if let Ok(permit) = permit {
            self.deliver(index, outcome, permit);
        }
    }

    /// Settles every request without a delivered outcome as a timeout.
    pub async fn time_out(&self, keys: &[CompileKey]) {
        let open: Vec<usize> = {
            let delivered = self.delivered.lock().expect("sink lock");
            (0..delivered.len()).filter(|&i| !delivered[i]).collect()
        };
        for index in open {
            let Ok(permit) = self.tx.reserve().await else {
                return;
            };
            let outcome = FetchOutcome::Miss {
                key: keys[index],
                reason: MissReason::Timeout,
            };
            self.deliver(index, outcome, permit);
        }
    }

    /// Sends the outcome through its reserved slot unless another outcome got there first.
    fn deliver(&self, index: usize, outcome: FetchOutcome, permit: mpsc::Permit<'_, FetchOutcome>) {
        let mut delivered = self.delivered.lock().expect("sink lock");
        if std::mem::replace(&mut delivered[index], true) {
            return;
        }
        if let FetchOutcome::Miss { reason, .. } = &outcome {
            self.count_failure(*reason);
        }
        permit.send(outcome);
    }
}

/// One `fetch` call.
pub(crate) struct Session {
    pub inner: Arc<Inner>,
    pub stage: Result<Arc<Stage>, std::io::ErrorKind>,
    pub durability: Durability,
    pub idle_timeout: Duration,
    pub keys: Vec<CompileKey>,
    /// Lookup ids named by the caller, one per key; otherwise derived from the keys.
    pub ids: Option<Vec<LookupId>>,
    pub expects: Vec<Option<Vec<ArtifactPath>>>,
    pub progress: Progress,
    pub cancel: Arc<AtomicBool>,
    pub sink: Sink,
    pub committer: Committer,
    pub refreshed: OnceCell<Option<Arc<RosterView>>>,
}

impl Session {
    pub async fn miss(&self, index: usize, reason: MissReason) {
        let outcome = FetchOutcome::Miss {
            key: self.keys[index],
            reason,
        };
        self.sink.settle(index, outcome, &self.progress).await;
    }
}

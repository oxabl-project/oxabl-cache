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
use crate::install::{Durability, Stage};
use crate::progress::Progress;

/// Delivers outcomes to the stream and remembers which requests are settled.
pub(crate) struct Sink {
    tx: mpsc::Sender<FetchOutcome>,
    settled: Mutex<Vec<bool>>,
}

impl Sink {
    pub fn new(tx: mpsc::Sender<FetchOutcome>, requests: usize) -> Self {
        Self {
            tx,
            settled: Mutex::new(vec![false; requests]),
        }
    }

    /// Delivers the outcome of request `index`, once; later outcomes for it are dropped.
    pub async fn settle(&self, index: usize, outcome: FetchOutcome, progress: &Progress) {
        {
            let mut settled = self.settled.lock().expect("sink lock");
            if std::mem::replace(&mut settled[index], true) {
                return;
            }
        }
        progress.touch();
        let _ = self.tx.send(outcome).await;
    }

    /// Settles every request still open as a timeout.
    pub async fn time_out(&self, keys: &[CompileKey]) {
        let open: Vec<usize> = {
            let mut settled = self.settled.lock().expect("sink lock");
            (0..settled.len())
                .filter(|&i| !std::mem::replace(&mut settled[i], true))
                .collect()
        };
        for index in open {
            let outcome = FetchOutcome::Miss {
                key: keys[index],
                reason: MissReason::Timeout,
            };
            let _ = self.tx.send(outcome).await;
        }
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

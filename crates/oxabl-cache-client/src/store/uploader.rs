use std::sync::Arc;

use oxabl_cache_format::{Namespace, Role};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle, JoinSet};

use super::dictionary::writer_dictionary;
use super::packer::{Queued, pack_all};
use super::seal::{SealContext, seal_set};
use super::shared::Shared;
use super::types::{ArtifactFileRef, StoreFailure, StoreOutcome, StoreReport};
use super::validate::validate;
use crate::activity::{Activity, ActivityKind};
use crate::cache::{Inner, permits_for};
use crate::compile_key::CompileKey;
use crate::error::CacheError;
use crate::error_log::{Tally, store_outcome_failure};
use crate::mode::Mode;

/// Sets that may be queued before `put` waits.
const QUEUED_SETS: usize = 10_000;
const SEALED_QUEUE: usize = 64;

struct Job {
    slot: usize,
    key: CompileKey,
    files: Vec<ArtifactFileRef>,
    plain: u64,
}

/// Ends the background work when the uploader is dropped without `finish`.
struct Guard(Vec<AbortHandle>);

impl Drop for Guard {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

/// A batched upload session, fed one artifact set at a time.
pub struct Uploader {
    inner: Arc<Inner>,
    started: std::time::Instant,
    shared: Arc<Shared>,
    jobs: mpsc::Sender<Job>,
    packer: JoinHandle<u64>,
    sealer: JoinHandle<()>,
    _guard: Guard,
}

async fn seal_job(
    context: Arc<SealContext>,
    shared: Arc<Shared>,
    job: Job,
    sealed: mpsc::Sender<Queued>,
) {
    let inner = &context.inner;
    let _plain = inner
        .gates
        .plain_bytes
        .clone()
        .acquire_many_owned(permits_for(job.plain))
        .await;
    let work = {
        let context = context.clone();
        tokio::task::spawn_blocking(move || seal_set(&context, job.slot, job.key, &job.files))
    };
    let slot = job.slot;
    let result = match work.await {
        Ok(result) => result,
        Err(_) => Err(StoreOutcome::Failed(StoreFailure::Rejected)),
    };
    drop(_plain);
    match result {
        Err(outcome) => shared.settle(slot, outcome),
        Ok(set) => {
            let budget = inner
                .gates
                .upload_bytes
                .clone()
                .acquire_many_owned(permits_for(set.segment.len() as u64))
                .await
                .expect("upload budget");
            if sealed
                .send(Queued {
                    sealed: set,
                    _budget: budget,
                })
                .await
                .is_err()
            {
                shared.settle(slot, StoreOutcome::Failed(StoreFailure::Unavailable));
            }
        }
    }
}

async fn seal_all(
    context: Arc<SealContext>,
    shared: Arc<Shared>,
    mut jobs: mpsc::Receiver<Job>,
    sealed: mpsc::Sender<Queued>,
) {
    let mut running = JoinSet::new();
    while let Some(job) = jobs.recv().await {
        if shared.stopped() {
            shared.settle(job.slot, StoreOutcome::Failed(StoreFailure::QuotaExceeded));
            continue;
        }
        let worker = context.inner.gates.workers.clone().acquire_owned().await;
        let (context, shared, sealed) = (context.clone(), shared.clone(), sealed.clone());
        running.spawn(async move {
            let _worker = worker;
            seal_job(context, shared, job, sealed).await;
        });
        while running.try_join_next().is_some() {}
    }
    while running.join_next().await.is_some() {}
}

impl Uploader {
    pub(crate) fn start(inner: Arc<Inner>) -> Result<Uploader, CacheError> {
        let view = inner.view();
        let namespace = match inner.mode {
            Mode::ReadOnly => return Err(CacheError::ReadOnly),
            Mode::Developer => Namespace::Personal,
            Mode::Ci => {
                if view.role(&inner.identity) != Some(Role::Ci) {
                    return Err(CacheError::NotAWriter);
                }
                Namespace::Trusted
            }
        };
        let shared = Arc::new(Shared::new(view.clone()));
        let (jobs, job_rx) = mpsc::channel(QUEUED_SETS);
        let (sealed_tx, sealed_rx) = mpsc::channel(SEALED_QUEUE);
        let packer = inner.handle.spawn(pack_all(
            inner.clone(),
            shared.clone(),
            namespace,
            sealed_rx,
        ));
        let sealer = {
            let (inner, shared) = (inner.clone(), shared.clone());
            inner.handle.clone().spawn(async move {
                let dictionary = writer_dictionary(&inner, &view).await;
                let context = Arc::new(SealContext {
                    inner,
                    view,
                    namespace,
                    dictionary,
                });
                seal_all(context, shared, job_rx, sealed_tx).await;
            })
        };
        let guard = Guard(vec![packer.abort_handle(), sealer.abort_handle()]);
        Ok(Uploader {
            inner,
            started: std::time::Instant::now(),
            shared,
            jobs,
            packer,
            sealer,
            _guard: guard,
        })
    }

    /// Queues one set; returns once queued, waiting only for back-pressure.
    pub async fn put(&self, key: CompileKey, files: Vec<ArtifactFileRef>) {
        let slot = self.shared.register(key);
        let plain = match validate(&files, self.inner.limits.plaintext_bytes()) {
            Ok(plain) => plain,
            Err(failure) => {
                self.shared.settle(slot, StoreOutcome::Failed(failure));
                return;
            }
        };
        if self.shared.stopped() {
            self.shared
                .settle(slot, StoreOutcome::Failed(StoreFailure::QuotaExceeded));
            return;
        }
        let job = Job {
            slot,
            key,
            files,
            plain,
        };
        if self.jobs.send(job).await.is_err() {
            self.shared
                .settle(slot, StoreOutcome::Failed(StoreFailure::Unavailable));
        }
    }

    /// Seals and uploads the rest, commits every pack, and reports per key.
    pub async fn finish(self) -> StoreReport {
        let Uploader {
            inner,
            started,
            shared,
            jobs,
            packer,
            sealer,
            _guard,
            ..
        } = self;
        drop(jobs);
        let _ = sealer.await;
        let bytes_uploaded = packer.await.unwrap_or(0);
        let outcomes: Vec<(CompileKey, StoreOutcome)> = shared
            .take()
            .into_iter()
            .map(|(key, outcome)| {
                (
                    key,
                    outcome.unwrap_or(StoreOutcome::Failed(StoreFailure::Unavailable)),
                )
            })
            .collect();
        let mut failures = Tally::default();
        for (_, outcome) in &outcomes {
            if let Some((kind, meaning)) = store_outcome_failure(outcome) {
                failures.add(kind, meaning);
            }
        }
        if !failures.is_empty() {
            failures.record(&inner.errors, "store", outcomes.len());
        }
        if !outcomes.is_empty() {
            record_push(&inner, &outcomes, bytes_uploaded, started);
        }
        StoreReport {
            outcomes,
            bytes_uploaded,
        }
    }
}

fn record_push(
    inner: &Arc<Inner>,
    outcomes: &[(CompileKey, StoreOutcome)],
    bytes: u64,
    started: std::time::Instant,
) {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let tally = |wanted: fn(&StoreOutcome) -> bool| {
        count(outcomes.iter().filter(|(_, o)| wanted(o)).count())
    };
    crate::activity::record(
        inner,
        Activity {
            at: crate::activity::now_secs(),
            kind: ActivityKind::Push,
            mode: inner.mode,
            requested: count(outcomes.len()),
            hits: tally(|o| matches!(o, StoreOutcome::Committed | StoreOutcome::Existed)),
            misses: tally(|o| matches!(o, StoreOutcome::Failed(StoreFailure::QuotaExceeded))),
            failed: tally(|o| {
                !matches!(
                    o,
                    StoreOutcome::Committed
                        | StoreOutcome::Existed
                        | StoreOutcome::Failed(StoreFailure::QuotaExceeded)
                )
            }),
            bytes,
            duration_ms: u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX),
            rtt_ms: 0,
        },
    );
}

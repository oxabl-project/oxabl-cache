use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::session::Session;
use super::types::{FetchOutcome, InstalledFile, MissReason, Provenance};
use crate::install::{Stage, StagedSet};

/// A batch is flushed when it holds this many sets, or this many bytes, or has waited this long.
const BATCH_SETS: usize = 128;
const BATCH_BYTES: u64 = 32 * 1024 * 1024;
const BATCH_WAIT: Duration = Duration::from_millis(20);

/// A verified set waiting, written to temporary files, for its batch.
pub(crate) struct Pending {
    pub global: usize,
    pub provenance: Provenance,
    pub set: StagedSet,
}

#[derive(Default)]
struct Batch {
    items: Vec<Pending>,
    bytes: u64,
    timer_armed: bool,
}

/// Makes batches of verified sets durable and then installs them, so that no file has its
/// final name before its bytes are on stable storage and no `Installed` precedes its files.
///
/// Per batch, in this order: one `syncfs` of the stage's file system (an fsync of every file
/// instead where `syncfs` fails, and of each file on another device), the renames of every
/// set, an fsync of every touched directory, then the `Installed` outcomes.
#[derive(Default)]
pub(crate) struct Committer {
    batch: Mutex<Batch>,
    gate: tokio::sync::Mutex<()>,
}

struct Finished {
    global: usize,
    provenance: Provenance,
    result: io::Result<Vec<InstalledFile>>,
}

fn make_durable(stage: &Stage, items: Vec<Pending>) -> Vec<Finished> {
    let device = stage.device();
    let covered = device.is_ok() && stage.sync_filesystem().is_ok();
    let mut finished = Vec::with_capacity(items.len());
    let mut renamed: Vec<(StagedSet, Finished)> = Vec::new();
    for item in items {
        let Pending {
            global,
            provenance,
            mut set,
        } = item;
        let synced = match (&device, covered) {
            (Ok(device), true) => set.sync_files_off_device(*device),
            _ => set.sync_files(),
        };
        match synced.and_then(|()| set.rename()) {
            Ok(files) => renamed.push((
                set,
                Finished {
                    global,
                    provenance,
                    result: Ok(files),
                },
            )),
            Err(e) => finished.push(Finished {
                global,
                provenance,
                result: Err(e),
            }),
        }
    }
    let mut seen = HashSet::new();
    for (set, mut done) in renamed {
        if let Err(e) = set.sync_dirs(&mut seen) {
            done.result = Err(e);
        }
        finished.push(done);
    }
    finished
}

impl Committer {
    /// Adds a set to the open batch, flushing it when it is full and arming a timer when it
    /// is the first.
    pub async fn submit(&self, session: &Arc<Session>, pending: Pending) {
        let (full, arm) = {
            let mut batch = self.batch.lock().expect("batch lock");
            batch.bytes += pending.set.bytes();
            batch.items.push(pending);
            let full = batch.items.len() >= BATCH_SETS || batch.bytes >= BATCH_BYTES;
            let arm = !full && !std::mem::replace(&mut batch.timer_armed, true);
            (full, arm)
        };
        if full {
            self.flush(session).await;
        } else if arm {
            let session = session.clone();
            session.inner.handle.clone().spawn(async move {
                tokio::time::sleep(BATCH_WAIT).await;
                session.committer.flush(&session).await;
            });
        }
    }

    /// Flushes the open batch and waits for every flush in flight.
    pub async fn flush(&self, session: &Arc<Session>) {
        let _turn = self.gate.lock().await;
        let items = {
            let mut batch = self.batch.lock().expect("batch lock");
            batch.bytes = 0;
            batch.timer_armed = false;
            std::mem::take(&mut batch.items)
        };
        if items.is_empty() || session.cancel.load(Ordering::Relaxed) {
            return;
        }
        let Ok(stage) = &session.stage else { return };
        let stage = stage.clone();
        session.progress.touch();
        let finished = tokio::task::spawn_blocking(move || make_durable(&stage, items))
            .await
            .unwrap_or_default();
        for done in finished {
            let outcome = match done.result {
                Ok(files) => FetchOutcome::Installed {
                    key: session.keys[done.global],
                    files,
                    provenance: done.provenance,
                },
                Err(e) => FetchOutcome::Miss {
                    key: session.keys[done.global],
                    reason: MissReason::Install(e.kind()),
                },
            };
            session
                .sink
                .settle(done.global, outcome, &session.progress)
                .await;
        }
    }
}

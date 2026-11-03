use std::sync::Arc;

use oxabl_cache_format::limits::PACK_SEGMENTS;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinSet;

use super::pack_build::{build, framing};
use super::publish::{Published, publish};
use super::seal::Sealed;
use super::shared::Shared;
use super::types::{StoreFailure, StoreOutcome};
use crate::cache::Inner;
use oxabl_cache_format::{Namespace, limits::PACK_BYTES};

/// A pack is closed once its segments reach this many bytes.
const TARGET_BYTES: u64 = 16 * 1024 * 1024;
const PACKS_IN_FLIGHT: usize = 8;

/// A sealed set queued for a pack, holding its share of the in-flight byte budget.
pub(crate) struct Queued {
    pub sealed: Sealed,
    pub _budget: OwnedSemaphorePermit,
}

#[derive(Default)]
struct Open {
    sets: Vec<Queued>,
    bytes: u64,
}

impl Open {
    fn fits(&self, add: u64) -> bool {
        self.bytes + add + framing(self.sets.len() + 1) <= PACK_BYTES
            && self.sets.len() < PACK_SEGMENTS
    }
}

async fn run_pack(
    inner: Arc<Inner>,
    shared: Arc<Shared>,
    namespace: Namespace,
    sets: Vec<Queued>,
    _turn: OwnedSemaphorePermit,
) -> u64 {
    let epoch = shared.view.epoch();
    let sealed: Vec<&Sealed> = sets.iter().map(|q| &q.sealed).collect();
    let built = match build(inner.pins.cache, namespace, epoch, &sealed) {
        Ok(built) => built,
        Err(failure) => {
            for q in &sets {
                shared.settle(q.sealed.slot, StoreOutcome::Failed(failure));
            }
            return 0;
        }
    };
    let name = match namespace {
        Namespace::Trusted => "trusted",
        Namespace::Personal => "personal",
    };
    match publish(&inner.api, name, &built, sets.len(), inner.checksum_sha256).await {
        Published::Done { outcomes, uploaded } => {
            for (q, outcome) in sets.iter().zip(outcomes) {
                shared.settle(q.sealed.slot, outcome);
            }
            uploaded
        }
        Published::Failed(failure) => {
            if failure == StoreFailure::QuotaExceeded {
                shared.stop();
            }
            for q in &sets {
                shared.settle(q.sealed.slot, StoreOutcome::Failed(failure));
            }
            0
        }
    }
}

/// Packs sealed sets as they arrive, closing a pack at 16 MiB or 2,000 segments (a segment
/// over 16 MiB gets its own), and publishes up to eight packs at once. Returns the bytes
/// uploaded once the input ends and every pack is done.
pub(crate) async fn pack_all(
    inner: Arc<Inner>,
    shared: Arc<Shared>,
    namespace: Namespace,
    mut queue: mpsc::Receiver<Queued>,
) -> u64 {
    let turns = Arc::new(Semaphore::new(PACKS_IN_FLIGHT));
    let mut running: JoinSet<u64> = JoinSet::new();
    let mut open = Open::default();
    let mut uploaded = 0u64;

    async fn close(
        open: &mut Open,
        running: &mut JoinSet<u64>,
        turns: &Arc<Semaphore>,
        inner: &Arc<Inner>,
        shared: &Arc<Shared>,
        namespace: Namespace,
    ) {
        if open.sets.is_empty() {
            return;
        }
        let sets = std::mem::take(&mut open.sets);
        open.bytes = 0;
        let turn = turns.clone().acquire_owned().await.expect("pack turns");
        running.spawn(run_pack(
            inner.clone(),
            shared.clone(),
            namespace,
            sets,
            turn,
        ));
    }

    while let Some(queued) = queue.recv().await {
        let size = queued.sealed.segment.len() as u64;
        if !open.fits(size) || (size > TARGET_BYTES && !open.sets.is_empty()) {
            close(&mut open, &mut running, &turns, &inner, &shared, namespace).await;
        }
        open.bytes += size;
        open.sets.push(queued);
        if open.bytes >= TARGET_BYTES || open.sets.len() >= PACK_SEGMENTS {
            close(&mut open, &mut running, &turns, &inner, &shared, namespace).await;
        }
        while let Some(done) = running.try_join_next() {
            uploaded += done.unwrap_or(0);
        }
    }
    close(&mut open, &mut running, &turns, &inner, &shared, namespace).await;
    while let Some(done) = running.join_next().await {
        uploaded += done.unwrap_or(0);
    }
    uploaded
}

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::types::StoreOutcome;
use crate::cache::RosterView;
use crate::compile_key::CompileKey;
use std::sync::Arc;

/// State of one upload session: the outcome of every set fed to it, and the roster it signs under.
pub(crate) struct Shared {
    pub view: Arc<RosterView>,
    outcomes: Mutex<Vec<(CompileKey, Option<StoreOutcome>)>>,
    stopped: AtomicBool,
}

impl Shared {
    pub fn new(view: Arc<RosterView>) -> Self {
        Self {
            view,
            outcomes: Mutex::new(Vec::new()),
            stopped: AtomicBool::new(false),
        }
    }

    /// Reserves the next slot for `key`.
    pub fn register(&self, key: CompileKey) -> usize {
        let mut outcomes = self.outcomes.lock().expect("outcome lock");
        outcomes.push((key, None));
        outcomes.len() - 1
    }

    /// Records the outcome of a slot; the first one wins.
    pub fn settle(&self, slot: usize, outcome: StoreOutcome) {
        let mut outcomes = self.outcomes.lock().expect("outcome lock");
        outcomes[slot].1.get_or_insert(outcome);
    }

    /// Stops accepting writes: the quota is used up.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    pub fn take(&self) -> Vec<(CompileKey, Option<StoreOutcome>)> {
        std::mem::take(&mut *self.outcomes.lock().expect("outcome lock"))
    }
}

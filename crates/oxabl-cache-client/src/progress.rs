//! A shared mark of the last time anything moved, for idle timeouts.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub(crate) struct Progress {
    start: Instant,
    last_ms: AtomicU64,
}

impl Progress {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            last_ms: AtomicU64::new(0),
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Records that bytes arrived or an outcome completed.
    pub fn touch(&self) {
        self.last_ms.store(self.now_ms(), Ordering::Relaxed);
    }

    pub fn idle_for(&self) -> Duration {
        Duration::from_millis(
            self.now_ms()
                .saturating_sub(self.last_ms.load(Ordering::Relaxed)),
        )
    }
}

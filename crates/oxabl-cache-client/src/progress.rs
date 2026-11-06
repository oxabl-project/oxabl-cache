//! A shared mark of the last time anything moved, for idle timeouts.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub(crate) struct Progress {
    start: Instant,
    last_ms: AtomicU64,
    waiting: AtomicUsize,
}

impl Progress {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            last_ms: AtomicU64::new(0),
            waiting: AtomicUsize::new(0),
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Records that bytes arrived or an outcome completed.
    pub fn touch(&self) {
        self.last_ms.store(self.now_ms(), Ordering::Relaxed);
    }

    /// Marks one sender as waiting for the reader. While any is, nothing is idle; the
    /// mark counts as progress again when it drops.
    pub fn waiting(&self) -> Waiting<'_> {
        self.waiting.fetch_add(1, Ordering::Relaxed);
        Waiting(self)
    }

    pub fn idle_for(&self) -> Duration {
        if self.waiting.load(Ordering::Relaxed) > 0 {
            return Duration::ZERO;
        }
        Duration::from_millis(
            self.now_ms()
                .saturating_sub(self.last_ms.load(Ordering::Relaxed)),
        )
    }
}

/// Held while a sender waits for room in the outcome stream.
pub(crate) struct Waiting<'a>(&'a Progress);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.touch();
        self.0.waiting.fetch_sub(1, Ordering::Relaxed);
    }
}

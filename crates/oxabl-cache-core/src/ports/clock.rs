//! The source of time.

use std::time::{SystemTime, UNIX_EPOCH};

/// Supplies the current time in unix seconds.
pub trait Clock: Send + Sync + 'static {
    /// The current time.
    fn now(&self) -> i64;
}

/// The operating-system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
            })
    }
}

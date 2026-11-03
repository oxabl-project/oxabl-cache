//! Request and byte counters of one cache handle.

use std::sync::atomic::{AtomicU64, Ordering};

/// A snapshot of what a [`crate::Cache`] has done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// HTTP requests sent, retries included.
    pub requests: u64,
    /// Response body bytes received.
    pub bytes_received: u64,
    /// Request body bytes sent.
    pub bytes_sent: u64,
}

#[derive(Default)]
pub(crate) struct Counters {
    requests: AtomicU64,
    received: AtomicU64,
    sent: AtomicU64,
}

impl Counters {
    pub fn request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn received(&self, bytes: u64) {
        self.received.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn sent(&self, bytes: u64) {
        self.sent.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> Stats {
        Stats {
            requests: self.requests.load(Ordering::Relaxed),
            bytes_received: self.received.load(Ordering::Relaxed),
            bytes_sent: self.sent.load(Ordering::Relaxed),
        }
    }
}

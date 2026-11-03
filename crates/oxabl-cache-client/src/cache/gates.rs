use std::sync::Arc;

use tokio::sync::Semaphore;

/// Bytes that may be in flight at once, per direction.
pub(crate) const BYTE_BUDGET: u64 = 256 * 1024 * 1024;
/// One permit stands for this many bytes.
pub(crate) const PERMIT_BYTES: u64 = 1024;

/// Permits a transfer of `bytes` needs, at least one and never more than the whole budget.
pub(crate) fn permits_for(bytes: u64) -> u32 {
    let permits = bytes
        .div_ceil(PERMIT_BYTES)
        .clamp(1, BYTE_BUDGET / PERMIT_BYTES);
    u32::try_from(permits).unwrap_or(u32::MAX)
}

/// The concurrency and memory bounds every operation of one cache handle shares.
pub(crate) struct Gates {
    /// Concurrent HTTP transfers.
    pub connections: Arc<Semaphore>,
    /// Sealed bytes being fetched or decoded, up to 256 MiB.
    pub fetch_bytes: Arc<Semaphore>,
    /// Plaintext bytes held for sealing, up to 256 MiB.
    pub plain_bytes: Arc<Semaphore>,
    /// Sealed bytes queued or uploading, up to 256 MiB.
    pub upload_bytes: Arc<Semaphore>,
    /// CPU workers.
    pub workers: Arc<Semaphore>,
}

impl Gates {
    pub fn new(connections: usize) -> Self {
        let budget = (BYTE_BUDGET / PERMIT_BYTES) as usize;
        let workers = std::thread::available_parallelism().map_or(4, usize::from);
        Self {
            connections: Arc::new(Semaphore::new(connections)),
            fetch_bytes: Arc::new(Semaphore::new(budget)),
            plain_bytes: Arc::new(Semaphore::new(budget)),
            upload_bytes: Arc::new(Semaphore::new(budget)),
            workers: Arc::new(Semaphore::new(workers)),
        }
    }
}

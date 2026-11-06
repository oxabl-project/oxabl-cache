//! Sealed telemetry reports: opaque to the server, readable by the cache's members.

/// One stored report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTelemetry {
    /// Position in the cache's report log; increases with every report.
    pub seq: u64,
    /// When the server received it, unix seconds.
    pub received_at: i64,
    /// The sealed report as the client sent it.
    pub body: Vec<u8>,
}

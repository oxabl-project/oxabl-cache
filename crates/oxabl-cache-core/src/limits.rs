//! Protocol limits shared by the use cases.

/// Lookup ids per request.
pub const LOOKUP_IDS: usize = 10_000;
/// Ids per sample request.
pub const SAMPLE_IDS: usize = 1_000;
/// Entries per commit.
pub const COMMIT_ENTRIES: usize = 2_000;
/// Segments per pack.
pub const SEGMENTS_PER_PACK: usize = 2_000;
/// Largest pack, a single PUT.
pub const PACK_BYTES: u64 = 32 * 1024 * 1024;
/// Smallest object that can hold a pack header and trailer.
pub const PACK_MIN_BYTES: u64 = 72;
/// Bytes before the first segment of a pack.
pub const PACK_HEADER_BYTES: u64 = 24;
/// Bytes after the footer of a pack.
pub const PACK_TRAILER_BYTES: u64 = 48;
/// Largest sealed record.
pub const RECORD_BYTES: usize = 4_096;
/// Presigned URL lifetime in seconds.
pub const URL_LIFETIME_SECS: i64 = 15 * 60;
/// Pending pack lease in seconds.
pub const LEASE_SECS: i64 = 60 * 60;
/// Largest roster body.
pub const ROSTER_BYTES: usize = 1024 * 1024;
/// Telemetry reports kept per cache; older ones are dropped first.
pub const TELEMETRY_KEEP: usize = 4_096;
/// Seconds a telemetry report is kept.
pub const TELEMETRY_SECS: i64 = 30 * 24 * 60 * 60;
/// Telemetry reports per read.
pub const TELEMETRY_PAGE: usize = 1_000;

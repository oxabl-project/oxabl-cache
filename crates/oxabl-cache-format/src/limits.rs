//! Size limits of the protocol, enforced by every decoder and encoder.

use crate::error::{FormatError, Result};

/// Lookup ids per request.
pub const LOOKUP_IDS: usize = 10_000;
/// Ids per sample request.
pub const SAMPLE_IDS: usize = 1_000;
/// Entries per commit.
pub const COMMIT_ENTRIES: usize = 2_000;
/// Segments per pack.
pub const PACK_SEGMENTS: usize = 2_000;
/// Pack object size in bytes.
pub const PACK_BYTES: u64 = 32 * 1024 * 1024;
/// Files per artifact set.
pub const FILES_PER_SET: usize = 16;
/// Length of one path in bytes.
pub const PATH_BYTES: usize = 1_024;
/// Total path bytes per artifact set.
pub const SET_PATH_BYTES: usize = 3_072;
/// Sealed record size in bytes.
pub const SEALED_RECORD_BYTES: usize = 4_096;
/// Sealed record lengths are multiples of this.
pub const RECORD_ALIGN: usize = 256;
/// Default plaintext per artifact set.
pub const DEFAULT_PLAINTEXT_BYTES: u64 = 64 * 1024 * 1024;
/// Upper bound of the configurable plaintext per artifact set.
pub const MAX_PLAINTEXT_BYTES: u64 = 1024 * 1024 * 1024;
/// Roster size in bytes.
pub const ROSTER_BYTES: usize = 1024 * 1024;
/// Members per roster.
pub const ROSTER_MEMBERS: usize = 1_000;
/// Signatures on a roster.
pub const ROSTER_SIGNATURES: usize = 4;
/// Characters in a Bech32 string.
pub const BECH32_CHARS: usize = 1_024;
/// Largest sealed telemetry report.
pub const TELEMETRY_BYTES: usize = 1_024;

/// Limits a reader applies that a deployment may configure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    plaintext_bytes: u64,
}

impl Limits {
    /// Limits with the given plaintext cap per artifact set, at most [`MAX_PLAINTEXT_BYTES`].
    pub fn new(plaintext_bytes: u64) -> Result<Self> {
        if plaintext_bytes > MAX_PLAINTEXT_BYTES {
            return Err(FormatError::LimitExceeded("plaintext per artifact set"));
        }
        Ok(Self { plaintext_bytes })
    }

    /// Plaintext cap per artifact set.
    pub fn plaintext_bytes(&self) -> u64 {
        self.plaintext_bytes
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            plaintext_bytes: DEFAULT_PLAINTEXT_BYTES,
        }
    }
}

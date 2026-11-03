//! Tunables of the use cases; the defaults are the documented ones.

use crate::domain::claim_key::ClaimKey;
use crate::limits::{LEASE_SECS, URL_LIFETIME_SECS};

/// Behaviour settings of the cache service.
#[derive(Debug, Clone)]
pub struct Config {
    /// Presigned URL lifetime in seconds.
    pub url_lifetime_secs: i64,
    /// Pending pack lease in seconds.
    pub lease_secs: i64,
    /// Time a dead pack stays before its bytes are deleted.
    pub grace_secs: i64,
    /// Compaction takes packs whose live share is below this many thousandths.
    pub compaction_live_permille: u64,
    /// Compaction skips packs smaller than this.
    pub compaction_min_pack_bytes: u64,
    /// Eviction starts above this share of the hard quota, in thousandths.
    pub evict_high_permille: u64,
    /// Eviction stops at or below this share of the hard quota, in thousandths.
    pub evict_low_permille: u64,
    /// One user's personal live and pending bytes, in thousandths of the hard quota.
    pub personal_cap_permille: u64,
    /// Stored bytes may not exceed this multiple of the hard quota.
    pub stored_ceiling_factor: u64,
    /// Report verifications per principal per window.
    pub report_limit: usize,
    /// Report rate-limit window in seconds.
    pub report_window_secs: i64,
    /// Entries removed per transaction by the background jobs.
    pub gc_batch: usize,
    /// Retention for new caches, in days.
    pub default_retention_days: u32,
    /// Hard quota for new caches, in bytes.
    pub default_hard_bytes: u64,
    /// Keys the hashes of OIDC policy claims; without it OIDC trust is not configured.
    pub claim_key: Option<ClaimKey>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url_lifetime_secs: URL_LIFETIME_SECS,
            lease_secs: LEASE_SECS,
            grace_secs: 24 * 60 * 60,
            compaction_live_permille: 500,
            compaction_min_pack_bytes: 4 * 1024 * 1024,
            evict_high_permille: 900,
            evict_low_permille: 800,
            personal_cap_permille: 100,
            stored_ceiling_factor: 3,
            report_limit: 10,
            report_window_secs: 60,
            gc_batch: 500,
            default_retention_days: 30,
            default_hard_bytes: 20 * 1024 * 1024 * 1024,
            claim_key: None,
        }
    }
}

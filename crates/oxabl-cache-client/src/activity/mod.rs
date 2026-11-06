//! What fetches and stores did: a local journal per cache on this machine, and sealed reports
//! shared with the cache's other members through the server, which cannot read them.

mod journal;
mod summary;
mod team;
mod upload;

use std::time::{SystemTime, UNIX_EPOCH};

use oxabl_cache_format::{TelemetryKind, TelemetryMode, TelemetryReport};
use serde::{Deserialize, Serialize};

use crate::mode::Mode;

pub use summary::ActivitySummary;

pub(crate) use journal::Journal;
pub(crate) use team::team_activity;
pub(crate) use upload::{record, upload_pending};

/// Whether an activity fetched or stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    /// A fetch: keys asked for, entries installed.
    Pull,
    /// A store: sets put, sets committed or already present.
    Push,
}

/// One fetch or store as the client that ran it measured it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    /// Unix seconds when it ended, on the measuring client's clock.
    pub at: u64,
    pub kind: ActivityKind,
    pub mode: Mode,
    /// Keys asked for, or sets put.
    pub requested: u32,
    /// Entries installed, or sets committed or already present.
    pub hits: u32,
    /// Keys the cache did not have, or sets refused for quota.
    pub misses: u32,
    /// Everything else that did not succeed.
    pub failed: u32,
    /// Plaintext bytes installed, or sealed bytes uploaded.
    pub bytes: u64,
    pub duration_ms: u32,
    /// Round trip of the server's health check, measured by the same client; 0 when unknown.
    pub rtt_ms: u32,
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Activity {
    pub(crate) fn report(&self, id: [u8; 16]) -> TelemetryReport {
        TelemetryReport {
            id,
            at: self.at,
            kind: match self.kind {
                ActivityKind::Pull => TelemetryKind::Pull,
                ActivityKind::Push => TelemetryKind::Push,
            },
            mode: match self.mode {
                Mode::Ci => TelemetryMode::Ci,
                Mode::Developer => TelemetryMode::Developer,
                Mode::ReadOnly => TelemetryMode::ReadOnly,
            },
            requested: self.requested,
            hits: self.hits,
            misses: self.misses,
            failed: self.failed,
            bytes: self.bytes,
            duration_ms: self.duration_ms,
            rtt_ms: self.rtt_ms,
        }
    }

    pub(crate) fn from_report(report: &TelemetryReport) -> Self {
        Self {
            at: report.at,
            kind: match report.kind {
                TelemetryKind::Pull => ActivityKind::Pull,
                TelemetryKind::Push => ActivityKind::Push,
            },
            mode: match report.mode {
                TelemetryMode::Ci => Mode::Ci,
                TelemetryMode::Developer => Mode::Developer,
                TelemetryMode::ReadOnly => Mode::ReadOnly,
            },
            requested: report.requested,
            hits: report.hits,
            misses: report.misses,
            failed: report.failed,
            bytes: report.bytes,
            duration_ms: report.duration_ms,
            rtt_ms: report.rtt_ms,
        }
    }
}

//! A small bounded log of what failed in recent runs, kept under the state directory.
//!
//! An entry holds a time, an operation, an error kind and a short fixed message. Messages are
//! built from error kinds and counts only: never a secret, a token, a path or file content.
//! Writing the log never fails the operation it records.

mod describe;
mod tally;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::state::bounded_log::{self, Bounds};

pub(crate) use describe::{
    cache_kind, dictionary_kind, fetch_failure, open_failure, store_outcome_failure,
};
pub(crate) use tally::Tally;

/// Entries and bytes kept; older entries are dropped first.
const BOUNDS: Bounds = Bounds {
    entries: 30,
    bytes: 4 * 1024,
};
const MAX_OPERATION: usize = 24;
const MAX_KIND: usize = 32;
const MAX_MESSAGE: usize = 160;

/// One recorded failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedError {
    /// Unix seconds.
    #[serde(rename = "t")]
    pub at: u64,
    /// What was attempted: `open`, `probe`, `fetch`, `store`, `dictionary` or `status`.
    #[serde(rename = "op")]
    pub operation: String,
    /// The kind of error, such as `unavailable` or `corrupt`.
    pub kind: String,
    /// A short description.
    #[serde(rename = "msg")]
    pub message: String,
}

fn clip(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '?'
            }
        })
        .collect()
}

impl LoggedError {
    fn new(operation: &str, kind: &str, message: &str) -> Self {
        Self {
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            operation: clip(operation, MAX_OPERATION),
            kind: clip(kind, MAX_KIND),
            message: clip(message, MAX_MESSAGE),
        }
    }
}

/// The log of one cache on this machine.
#[derive(Debug, Clone)]
pub(crate) struct ErrorLog {
    path: Option<PathBuf>,
}

impl ErrorLog {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// Appends one failure; any problem with the file is ignored.
    pub fn record(&self, operation: &str, kind: &str, message: &str) {
        self.record_all(&[(operation, kind, message.to_owned())]);
    }

    /// Appends failures in one write; any problem with the file is ignored.
    pub fn record_all(&self, failures: &[(&str, &str, String)]) {
        let Some(path) = &self.path else { return };
        if failures.is_empty() {
            return;
        }
        let entries: Vec<LoggedError> = failures
            .iter()
            .map(|(operation, kind, message)| LoggedError::new(operation, kind, message))
            .collect();
        let _ = bounded_log::update(path, BOUNDS, |kept| kept.extend(entries));
    }

    /// The last `limit` failures, newest first. A missing or unreadable log is empty.
    pub fn recent(&self, limit: usize) -> Vec<LoggedError> {
        let Some(path) = &self.path else {
            return Vec::new();
        };
        let mut entries: Vec<LoggedError> = bounded_log::read(path, BOUNDS).unwrap_or_default();
        entries.reverse();
        entries.truncate(limit);
        entries
    }
}

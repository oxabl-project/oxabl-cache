use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::Activity;
use crate::state::bounded_log::{self, Bounds};

const BOUNDS: Bounds = Bounds {
    entries: 1_000,
    bytes: 320 * 1024,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub id: String,
    #[serde(default)]
    pub sent: bool,
    #[serde(flatten)]
    pub activity: Activity,
}

/// The activities of one cache on this machine, newest last. Writing never fails the operation
/// it records.
#[derive(Debug, Clone)]
pub(crate) struct Journal {
    path: Option<PathBuf>,
}

pub(crate) fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn unhex(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 {
        return None;
    }
    let mut id = [0u8; 16];
    for (i, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(id)
}

impl Journal {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    pub fn add(&self, id: [u8; 16], activity: Activity) {
        let Some(path) = &self.path else { return };
        let entry = Entry {
            id: hex(&id),
            sent: false,
            activity,
        };
        let _ = bounded_log::update(path, BOUNDS, |kept| kept.push(entry));
    }

    /// Every entry, oldest first; a missing or unreadable journal is empty.
    pub fn entries(&self) -> Vec<Entry> {
        self.path
            .as_ref()
            .and_then(|path| bounded_log::read(path, BOUNDS).ok())
            .unwrap_or_default()
    }

    pub fn mark_sent(&self, ids: &[String]) {
        let Some(path) = &self.path else { return };
        let _ = bounded_log::update(path, BOUNDS, |kept: &mut Vec<Entry>| {
            for entry in kept.iter_mut().filter(|e| ids.contains(&e.id)) {
                entry.sent = true;
            }
        });
    }
}

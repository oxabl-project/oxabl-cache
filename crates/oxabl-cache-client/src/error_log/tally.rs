use std::collections::BTreeMap;

use super::ErrorLog;

/// Failures of one run counted per kind, so that a run over thousands of entries logs one
/// line per kind.
#[derive(Default)]
pub(crate) struct Tally {
    kinds: BTreeMap<String, (u64, &'static str)>,
}

impl Tally {
    pub fn add(&mut self, kind: impl Into<String>, meaning: &'static str) {
        self.kinds.entry(kind.into()).or_insert((0, meaning)).0 += 1;
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// Logs one line per kind: how many of `total` entries failed and what that means.
    pub fn record(&self, log: &ErrorLog, operation: &str, total: usize) {
        let failures: Vec<(&str, &str, String)> = self
            .kinds
            .iter()
            .map(|(kind, (count, meaning))| {
                (
                    operation,
                    kind.as_str(),
                    format!("{count} of {total} entries {meaning}"),
                )
            })
            .collect();
        log.record_all(&failures);
    }
}

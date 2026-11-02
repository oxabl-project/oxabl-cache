//! Per-principal limit on report verifications.

use crate::ids::PrincipalId;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

/// A sliding window of verification times per principal.
#[derive(Debug, Default)]
pub struct ReportLimiter {
    windows: Mutex<HashMap<PrincipalId, VecDeque<i64>>>,
}

impl ReportLimiter {
    /// Admits a verification at `now`, or returns the seconds until one frees up.
    pub(crate) fn admit(
        &self,
        principal: PrincipalId,
        now: i64,
        limit: usize,
        window_secs: i64,
    ) -> Result<(), u64> {
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        let times = windows.entry(principal).or_default();
        while times.front().is_some_and(|t| *t <= now - window_secs) {
            times.pop_front();
        }
        if times.len() >= limit {
            let oldest = times.front().copied().unwrap_or(now);
            let wait = (oldest + window_secs - now).max(1);
            return Err(u64::try_from(wait).unwrap_or(1));
        }
        times.push_back(now);
        Ok(())
    }
}

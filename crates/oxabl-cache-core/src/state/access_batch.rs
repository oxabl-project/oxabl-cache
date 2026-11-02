//! Entry reads gathered between flushes, so a hit touches the database once a day.

use crate::domain::namespace::NamespaceKey;
use crate::ids::{CacheId, LookupId};
use std::collections::HashMap;
use std::sync::Mutex;

type EntryKey = (CacheId, NamespaceKey, LookupId);

/// Pending access-day updates.
#[derive(Debug, Default)]
pub struct AccessBatch {
    pending: Mutex<HashMap<EntryKey, i64>>,
}

impl AccessBatch {
    pub(crate) fn record(&self, cache: CacheId, namespace: NamespaceKey, id: LookupId, day: i64) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let slot = pending.entry((cache, namespace, id)).or_insert(day);
        *slot = (*slot).max(day);
    }

    pub(crate) fn take(&self) -> HashMap<EntryKey, i64> {
        std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub(crate) fn restore(&self, taken: HashMap<EntryKey, i64>) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        for (key, day) in taken {
            let slot = pending.entry(key).or_insert(day);
            *slot = (*slot).max(day);
        }
    }
}

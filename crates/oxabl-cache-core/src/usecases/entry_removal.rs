//! Deleting entries and releasing what only they kept alive.

use crate::domain::entry::EntryRef;
use crate::error::Result;
use crate::ids::{CacheId, SegmentId};
use crate::ports::MetaTx;
use crate::usecases::segment_death::{kill_segment, release_dictionary};
use std::collections::BTreeSet;

/// What a removal freed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Removal {
    /// Entries deleted.
    pub entries: u64,
    /// Live bytes released.
    pub freed_bytes: u64,
}

/// Deletes the entries in order, killing the artifact segment each one names and
/// releasing dictionaries nothing references any more. Stops early once
/// `enough(freed_bytes)` holds after an entry.
pub(crate) fn remove_entries(
    tx: &mut dyn MetaTx,
    cache: &CacheId,
    entries: &[EntryRef],
    now: i64,
    mut enough: impl FnMut(u64) -> bool,
) -> Result<Removal> {
    let mut removal = Removal::default();
    let mut dictionaries: BTreeSet<SegmentId> = BTreeSet::new();
    for entry in entries {
        if !tx.delete_entry(cache, &entry.namespace, &entry.lookup_id)? {
            continue;
        }
        removal.entries += 1;
        removal.freed_bytes += kill_segment(tx, cache, &entry.segment_id, now)?;
        if let Some(dictionary) = entry.dict_id {
            dictionaries.insert(dictionary);
        }
        if enough(removal.freed_bytes) {
            break;
        }
    }
    for dictionary in dictionaries {
        removal.freed_bytes += release_dictionary(tx, cache, &dictionary, now)?;
    }
    Ok(removal)
}

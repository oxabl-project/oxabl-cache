//! Entry rows.

use crate::domain::entry::{Entry, EntryHit, EntryRef};
use crate::domain::namespace::NamespaceKey;
use crate::error::Result;
use crate::ids::{CacheId, LookupId, SegmentId};

/// Which namespaces an eviction candidate list covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionScope {
    /// Every personal namespace.
    Personal,
    /// The trusted namespace.
    Trusted,
}

/// Queries and writes of entry rows.
pub trait EntriesTx {
    /// Inserts an entry and its access row; `false`, changing nothing, if the key exists.
    fn insert_entry(&mut self, entry: &Entry, access_day: i64) -> Result<bool>;
    /// The entries of `ids` in `namespaces` whose segment is live, with the
    /// record only when `with_record`.
    fn lookup_entries(
        &self,
        cache: &CacheId,
        namespaces: &[NamespaceKey],
        ids: &[LookupId],
        with_record: bool,
    ) -> Result<Vec<EntryHit>>;
    /// Raises the access day of entries whose day is lower.
    fn touch_entries(
        &mut self,
        cache: &CacheId,
        day: i64,
        keys: &[(NamespaceKey, LookupId)],
    ) -> Result<()>;
    /// The entries read longest ago first, within a scope.
    fn oldest_entries(
        &self,
        cache: &CacheId,
        scope: EvictionScope,
        limit: usize,
    ) -> Result<Vec<EntryRef>>;
    /// Entries last read before `before_day`.
    fn entries_idle_before(
        &self,
        cache: &CacheId,
        before_day: i64,
        limit: usize,
    ) -> Result<Vec<EntryRef>>;
    /// Entries of epochs below `epoch`.
    fn entries_below_epoch(
        &self,
        cache: &CacheId,
        epoch: u32,
        limit: usize,
    ) -> Result<Vec<EntryRef>>;
    /// Entries of one namespace.
    fn entries_of_namespace(
        &self,
        cache: &CacheId,
        namespace: &NamespaceKey,
        limit: usize,
    ) -> Result<Vec<EntryRef>>;
    /// Entries that name a segment as their own or as their dictionary.
    fn entries_naming_segment(&self, cache: &CacheId, segment: &SegmentId)
    -> Result<Vec<EntryRef>>;
    /// Whether any entry names the segment as its dictionary.
    fn dictionary_in_use(&self, cache: &CacheId, segment: &SegmentId) -> Result<bool>;
    /// Deletes an entry and its access row; `false` if it does not exist.
    fn delete_entry(
        &mut self,
        cache: &CacheId,
        namespace: &NamespaceKey,
        lookup_id: &LookupId,
    ) -> Result<bool>;
}

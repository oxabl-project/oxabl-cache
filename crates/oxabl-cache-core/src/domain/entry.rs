//! Entries and their commit outcomes.

use crate::domain::namespace::NamespaceKey;
use crate::ids::{CacheId, LookupId, PackId, SegmentId};

/// An immutable index row: a lookup id in a namespace names a sealed record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The cache the entry belongs to.
    pub cache: CacheId,
    /// The namespace.
    pub namespace: NamespaceKey,
    /// The keyed lookup id.
    pub lookup_id: LookupId,
    /// The artifact segment the entry names.
    pub segment_id: SegmentId,
    /// The dictionary the segment was compressed with.
    pub dict_id: Option<SegmentId>,
    /// Generation of the cache root key.
    pub epoch: u32,
    /// The sealed record, opaque to the server.
    pub record: Vec<u8>,
    /// Commit time, unix seconds.
    pub created_at: i64,
}

/// An entry as declared by a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrySpec {
    /// The keyed lookup id.
    pub lookup_id: LookupId,
    /// Index into the commit's segment table.
    pub segment_index: u32,
    /// The dictionary the segment was compressed with.
    pub dict_id: Option<SegmentId>,
    /// Generation of the cache root key.
    pub epoch: u32,
    /// The sealed record.
    pub record: Vec<u8>,
    /// False when the frame layer found the record header malformed.
    pub header_ok: bool,
}

/// The per-entry outcome of a commit; the numbers are the wire values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntryStatus {
    /// Inserted.
    Committed = 0,
    /// Another write won; the segment is dead.
    Exists = 1,
    /// Malformed, wrong epoch, bad segment reference or a segment id already recorded.
    Invalid = 2,
    /// The dictionary is not a live dictionary of this cache.
    MissingDictionary = 3,
}

impl EntryStatus {
    /// The wire value.
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Reads a wire value.
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Committed),
            1 => Some(Self::Exists),
            2 => Some(Self::Invalid),
            3 => Some(Self::MissingDictionary),
            _ => None,
        }
    }
}

/// An entry as removal and eviction see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRef {
    /// The namespace.
    pub namespace: NamespaceKey,
    /// The keyed lookup id.
    pub lookup_id: LookupId,
    /// The artifact segment the entry names.
    pub segment_id: SegmentId,
    /// The dictionary the entry names, if any.
    pub dict_id: Option<SegmentId>,
    /// Day the entry was last read.
    pub access_day: i64,
}

/// A hit of a lookup, with the location of its segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryHit {
    /// The namespace the entry lives in.
    pub namespace: NamespaceKey,
    /// The pack holding the segment.
    pub pack: PackId,
    /// Absolute offset of the segment.
    pub offset: u64,
    /// Length of the segment.
    pub length: u32,
    /// The dictionary the entry names, if any.
    pub dict_id: Option<SegmentId>,
    /// The sealed record, in fetch mode.
    pub record: Option<Vec<u8>>,
    /// Day the entry was last read.
    pub access_day: i64,
    /// The keyed lookup id.
    pub lookup_id: LookupId,
}

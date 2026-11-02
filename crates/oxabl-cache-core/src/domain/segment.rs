//! Segments.

use crate::ids::{CacheId, PackId, SegmentId};

/// What a segment holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    /// One sealed artifact set, named by exactly one entry.
    Artifact,
    /// A compression dictionary, named by the pointer and by entries.
    Dictionary,
}

/// A sealed unit of bytes at a location in a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The cache the segment belongs to.
    pub cache: CacheId,
    /// BLAKE3 of the sealed bytes.
    pub id: SegmentId,
    /// The pack holding the bytes.
    pub pack: PackId,
    /// Absolute offset in the pack.
    pub offset: u64,
    /// Length in bytes.
    pub length: u32,
    /// What it holds.
    pub kind: SegmentKind,
    /// Creation time, unix seconds.
    pub created_at: i64,
    /// Set once the segment is no longer named; never cleared.
    pub dead_since: Option<i64>,
}

/// A segment as declared by a commit, before it is recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSpec {
    /// BLAKE3 of the sealed bytes.
    pub id: SegmentId,
    /// Absolute offset in the pack.
    pub offset: u64,
    /// Length in bytes.
    pub length: u32,
    /// What it holds.
    pub kind: SegmentKind,
}

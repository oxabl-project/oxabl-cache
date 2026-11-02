//! Building pack objects from segments.

use crate::error::Result;
use crate::ids::{CacheId, SegmentId};

/// A segment's sealed bytes headed for a new pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackPart {
    /// The segment id.
    pub id: SegmentId,
    /// The sealed bytes.
    pub bytes: Vec<u8>,
}

/// Where a segment landed in an assembled pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// The segment id.
    pub id: SegmentId,
    /// Absolute offset in the pack.
    pub offset: u64,
    /// Length in bytes.
    pub length: u32,
}

/// A complete pack object and the placement of its segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledPack {
    /// The object bytes: header, segments, footer and trailer.
    pub bytes: Vec<u8>,
    /// One placement per part, in order.
    pub placements: Vec<Placement>,
}

/// Writes the pack byte format; compaction uses it to copy segments without keys.
pub trait PackAssembler: Send + Sync + 'static {
    /// Lays the parts out back to back with a header and a footer index.
    fn assemble(&self, cache: &CacheId, parts: Vec<PackPart>) -> Result<AssembledPack>;
}

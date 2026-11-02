//! `OXCM` commit request.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::{read_header, write_header};
use crate::ids::{LookupId, SegmentId};
use crate::limits::{COMMIT_ENTRIES, PACK_BYTES, PACK_SEGMENTS};
use crate::namespace::Namespace;
use crate::writer::{Writer, len32};

const MAGIC: [u8; 4] = *b"OXCM";

/// What a segment of the pack holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Artifact,
    Dictionary,
}

impl SegmentKind {
    fn to_u8(self) -> u8 {
        match self {
            Self::Artifact => 0,
            Self::Dictionary => 1,
        }
    }

    fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::Artifact),
            1 => Ok(Self::Dictionary),
            _ => Err(FormatError::Invalid("segment kind")),
        }
    }
}

/// One row of the segment table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSegment {
    pub segment_id: SegmentId,
    pub offset: u64,
    pub length: u32,
    pub kind: SegmentKind,
}

/// One entry: a lookup id, the segment holding its artifacts and its sealed record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitEntry<'a> {
    pub lookup_id: LookupId,
    pub segment_idx: u32,
    pub dict_id: Option<SegmentId>,
    pub epoch: u32,
    /// Sealed record bytes, opaque at this layer.
    pub record: &'a [u8],
}

/// A commit request for one pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRequest<'a> {
    pub ns: Namespace,
    pub pack_size: u64,
    pub segments: Vec<CommitSegment>,
    pub entries: Vec<CommitEntry<'a>>,
}

const SEGMENT_LEN: usize = 32 + 8 + 4 + 1;
const ENTRY_MIN: usize = 32 + 4 + 32 + 4 + 2;
const ZERO_ID: [u8; 32] = [0; 32];

impl<'a> CommitRequest<'a> {
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.check()?;
        let mut w = Writer::default();
        write_header(&mut w, MAGIC, 0);
        w.u8(self.ns.to_u8());
        w.u8(0);
        w.u64(self.pack_size);
        w.u32(len32(self.segments.len(), "segments")?);
        for s in &self.segments {
            w.bytes(&s.segment_id.0);
            w.u64(s.offset);
            w.u32(s.length);
            w.u8(s.kind.to_u8());
        }
        w.u32(len32(self.entries.len(), "entries")?);
        for e in &self.entries {
            w.bytes(&e.lookup_id.0);
            w.u32(e.segment_idx);
            w.bytes(&e.dict_id.map_or(ZERO_ID, |d| d.0));
            w.u32(e.epoch);
            w.record(e.record)?;
        }
        Ok(w.into_vec())
    }

    pub fn decode(bytes: &'a [u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        read_header(&mut c, MAGIC, 0)?;
        let ns = Namespace::from_u8(c.u8()?)?;
        c.reserved_u8()?;
        let pack_size = c.u64()?;
        let seg_count = c.count32(PACK_SEGMENTS, "segments", SEGMENT_LEN)?;
        let mut segments = Vec::with_capacity(seg_count);
        for _ in 0..seg_count {
            segments.push(CommitSegment {
                segment_id: SegmentId(c.array()?),
                offset: c.u64()?,
                length: c.u32()?,
                kind: SegmentKind::from_u8(c.u8()?)?,
            });
        }
        let entry_count = c.count32(COMMIT_ENTRIES, "entries", ENTRY_MIN)?;
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let lookup_id = LookupId(c.array()?);
            let segment_idx = c.u32()?;
            let dict: [u8; 32] = c.array()?;
            entries.push(CommitEntry {
                lookup_id,
                segment_idx,
                dict_id: (dict != ZERO_ID).then_some(SegmentId(dict)),
                epoch: c.u32()?,
                record: c.record()?,
            });
        }
        c.finish()?;
        let request = Self {
            ns,
            pack_size,
            segments,
            entries,
        };
        request.check()?;
        Ok(request)
    }

    fn check(&self) -> Result<()> {
        if self.pack_size > PACK_BYTES {
            return Err(FormatError::LimitExceeded("pack size"));
        }
        if self.segments.len() > PACK_SEGMENTS {
            return Err(FormatError::LimitExceeded("segments"));
        }
        if self.entries.len() > COMMIT_ENTRIES {
            return Err(FormatError::LimitExceeded("entries"));
        }
        if self.ns == Namespace::Personal
            && self
                .segments
                .iter()
                .any(|s| s.kind == SegmentKind::Dictionary)
        {
            return Err(FormatError::Invalid(
                "dictionary segment in a personal commit",
            ));
        }
        if self
            .entries
            .iter()
            .any(|e| e.dict_id.is_some_and(|d| d.0 == ZERO_ID))
        {
            return Err(FormatError::Invalid("zero dict_id"));
        }
        Ok(())
    }
}

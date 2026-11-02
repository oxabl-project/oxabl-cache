//! `OXPK` pack footer: the segment index.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::ids::SegmentId;
use crate::limits::PACK_SEGMENTS;
use crate::pack_header::PACK_HEADER_LEN;
use crate::pack_trailer::PackTrailer;
use crate::segment::MIN_SEGMENT_LEN;
use crate::writer::{Writer, len32};

/// Length of one footer entry.
pub const FOOTER_ENTRY_LEN: usize = 32 + 8 + 4;

/// Where one segment sits in the pack object. Offsets are absolute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackSegment {
    pub segment_id: SegmentId,
    pub offset: u64,
    pub length: u32,
}

/// The footer of a pack object. Segments are contiguous from the end of the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFooter {
    pub segments: Vec<PackSegment>,
}

impl PackFooter {
    /// Offset the footer starts at: the end of the last segment.
    pub fn footer_offset(&self) -> Result<u64> {
        check_layout(&self.segments)
    }

    /// The trailer for this footer. `hash` is BLAKE3.
    pub fn trailer(&self, hash: impl FnOnce(&[u8]) -> [u8; 32]) -> Result<PackTrailer> {
        let footer_offset = self.footer_offset()?;
        let bytes = self.encode()?;
        let trailer = PackTrailer {
            segment_count: len32(self.segments.len(), "segments")?,
            footer_offset,
            footer_digest: hash(&bytes),
        };
        trailer.check()?;
        Ok(trailer)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        check_layout(&self.segments)?;
        let mut w = Writer::with_capacity(self.segments.len() * FOOTER_ENTRY_LEN);
        for s in &self.segments {
            w.bytes(&s.segment_id.0);
            w.u64(s.offset);
            w.u32(s.length);
        }
        Ok(w.into_vec())
    }

    /// Decodes the footer bytes that `trailer` describes. `hash` is BLAKE3; the footer is
    /// rejected unless it hashes to the trailer's digest.
    pub fn decode(
        bytes: &[u8],
        trailer: &PackTrailer,
        hash: impl FnOnce(&[u8]) -> [u8; 32],
    ) -> Result<Self> {
        trailer.check()?;
        let count = trailer.segment_count as usize;
        if bytes.len() != count * FOOTER_ENTRY_LEN {
            return Err(FormatError::Invalid("footer length"));
        }
        if hash(bytes) != trailer.footer_digest {
            return Err(FormatError::Invalid("footer digest"));
        }
        let mut c = Cursor::new(bytes);
        let mut segments = Vec::with_capacity(count);
        for _ in 0..count {
            segments.push(PackSegment {
                segment_id: SegmentId(c.array()?),
                offset: c.u64()?,
                length: c.u32()?,
            });
        }
        c.finish()?;
        if check_layout(&segments)? != trailer.footer_offset {
            return Err(FormatError::Invalid("footer offset"));
        }
        Ok(Self { segments })
    }
}

fn check_layout(segments: &[PackSegment]) -> Result<u64> {
    if segments.len() > PACK_SEGMENTS {
        return Err(FormatError::LimitExceeded("segments"));
    }
    let mut next = PACK_HEADER_LEN as u64;
    for s in segments {
        if s.offset != next || (s.length as usize) < MIN_SEGMENT_LEN {
            return Err(FormatError::Invalid("segment layout"));
        }
        next = next
            .checked_add(u64::from(s.length))
            .ok_or(FormatError::Invalid("segment layout"))?;
    }
    Ok(next)
}

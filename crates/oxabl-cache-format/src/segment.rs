//! `OXSG` sealed segment: the outer header and the chunk structure of the body.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::VERSION;
use crate::limits::PACK_BYTES;
use crate::writer::Writer;

const MAGIC: [u8; 4] = *b"OXSG";
const ALG_AES256GCM_STREAM: u8 = 1;
const CHUNK_LOG2: u8 = 16;

/// Length of the segment header.
pub const SEGMENT_HEADER_LEN: usize = 72;
/// Plaintext bytes in a full chunk.
pub const CHUNK_PLAINTEXT_LEN: usize = 1 << CHUNK_LOG2;
/// Length of the authentication tag that ends every chunk.
pub const CHUNK_TAG_LEN: usize = 16;
/// Smallest segment: a header and one chunk holding the 14-byte plaintext header.
pub const MIN_SEGMENT_LEN: usize = SEGMENT_HEADER_LEN + 14 + CHUNK_TAG_LEN;

/// The unsealed header of a segment. Its bytes are the AEAD associated data's input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentHeader {
    pub salt: [u8; 32],
    pub k_commit: [u8; 32],
}

impl SegmentHeader {
    pub fn encode(&self) -> [u8; SEGMENT_HEADER_LEN] {
        let mut w = Writer::with_capacity(SEGMENT_HEADER_LEN);
        w.bytes(&MAGIC);
        w.u8(VERSION);
        w.u8(ALG_AES256GCM_STREAM);
        w.u8(CHUNK_LOG2);
        w.u8(0);
        w.bytes(&self.salt);
        w.bytes(&self.k_commit);
        let mut out = [0u8; SEGMENT_HEADER_LEN];
        out.copy_from_slice(&w.into_vec());
        out
    }

    fn read(c: &mut Cursor<'_>) -> Result<Self> {
        if c.array::<4>()? != MAGIC {
            return Err(FormatError::BadMagic);
        }
        let version = c.u8()?;
        if version != VERSION {
            return Err(FormatError::UnsupportedVersion(version));
        }
        if c.u8()? != ALG_AES256GCM_STREAM {
            return Err(FormatError::Invalid("segment algorithm"));
        }
        if c.u8()? != CHUNK_LOG2 {
            return Err(FormatError::Invalid("chunk size"));
        }
        c.reserved_u8()?;
        Ok(Self {
            salt: c.array()?,
            k_commit: c.array()?,
        })
    }

    /// Decodes exactly one header.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        let header = Self::read(&mut c)?;
        c.finish()?;
        Ok(header)
    }
}

/// A sealed segment: header and body. The body is chunks of ciphertext and tags, opaque here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedSegment<'a> {
    pub header: SegmentHeader,
    pub body: &'a [u8],
}

impl<'a> SealedSegment<'a> {
    pub fn encode(&self) -> Result<Vec<u8>> {
        plaintext_len(self.body.len())?;
        let mut out = Vec::with_capacity(SEGMENT_HEADER_LEN + self.body.len());
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(self.body);
        Ok(out)
    }

    pub fn decode(bytes: &'a [u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        let header = SegmentHeader::read(&mut c)?;
        let body = c.take(c.remaining())?;
        plaintext_len(body.len())?;
        Ok(Self { header, body })
    }
}

/// Plaintext length of a body of `body_len` bytes: every chunk but the last is full and
/// the last holds at least one plaintext byte.
pub fn plaintext_len(body_len: usize) -> Result<u64> {
    if body_len as u64 > PACK_BYTES {
        return Err(FormatError::LimitExceeded("segment size"));
    }
    let full = CHUNK_PLAINTEXT_LEN + CHUNK_TAG_LEN;
    let chunks = body_len.div_ceil(full);
    let last = body_len - chunks.saturating_sub(1) * full;
    if chunks == 0 || last <= CHUNK_TAG_LEN {
        return Err(FormatError::Invalid("chunk structure"));
    }
    Ok((body_len - chunks * CHUNK_TAG_LEN) as u64)
}

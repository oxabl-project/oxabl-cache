//! What sealing a segment reports, and the size a sealed segment will have.

use oxabl_cache_format::SegmentId;
use oxabl_cache_format::padme::padme;
use oxabl_cache_format::segment::{CHUNK_PLAINTEXT_LEN, CHUNK_TAG_LEN, SEGMENT_HEADER_LEN};

use crate::error::{Error, Result};

const PLAINTEXT_HEADER_LEN: u64 = 14;

/// Result of sealing one segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSeal {
    /// BLAKE3 of the sealed bytes.
    pub id: SegmentId,
    /// Length of the sealed bytes.
    pub sealed_len: u64,
    /// Bytes read from the input before compression; zero for [`crate::seal_compressed`].
    pub plain_len: u64,
    /// Length of the compressed stream inside the plaintext.
    pub compressed_len: u64,
}

/// Length of the sealed segment that carries a compressed stream of `compressed_len` bytes:
/// the header, the Padmé-padded plaintext and one tag per chunk.
pub fn sealed_segment_len(compressed_len: u64) -> Result<u64> {
    let padded = compressed_len
        .checked_add(PLAINTEXT_HEADER_LEN)
        .and_then(padme)
        .ok_or(Error::Limit("segment size"))?;
    let chunks = padded.div_ceil(CHUNK_PLAINTEXT_LEN as u64);
    padded
        .checked_add(chunks * CHUNK_TAG_LEN as u64)
        .and_then(|n| n.checked_add(SEGMENT_HEADER_LEN as u64))
        .ok_or(Error::Limit("segment size"))
}

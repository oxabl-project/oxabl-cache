//! Sealing: zstd, Padmé padding and AES-256-GCM STREAM into an `OXSG` segment.

use std::io::{Read, Write};

use oxabl_cache_format::limits::PACK_BYTES;
use oxabl_cache_format::segment::{CHUNK_PLAINTEXT_LEN, CHUNK_TAG_LEN, SegmentHeader};
use oxabl_cache_format::{Codec, Limits, SegmentId, SegmentPlaintext};
use zeroize::Zeroizing;

use crate::aead::Cipher;
use crate::compress::compress;
use crate::dictionary::Dictionary;
use crate::error::{Error, Result};
use crate::keys::SegmentKey;
use crate::nonce::chunk_nonce;
use crate::random;
use crate::segment_info::{SegmentSeal, sealed_segment_len};

/// zstd level used unless the caller chooses another.
pub const DEFAULT_ZSTD_LEVEL: i32 = 3;

/// How a segment is compressed.
#[derive(Clone, Copy)]
pub struct SealOptions<'a> {
    /// zstd level; ignored when a dictionary is set, which was prepared at its own level.
    pub level: i32,
    pub dictionary: Option<&'a Dictionary>,
}

impl Default for SealOptions<'_> {
    fn default() -> Self {
        Self {
            level: DEFAULT_ZSTD_LEVEL,
            dictionary: None,
        }
    }
}

/// Compresses `input` (the concatenated files of an artifact set), pads and seals it, and
/// writes the `OXSG` bytes to `out`. Memory is bounded by the compressed size plus one chunk.
pub fn seal_segment(
    key: &SegmentKey,
    input: impl Read,
    options: &SealOptions<'_>,
    limits: &Limits,
    out: impl Write,
) -> Result<SegmentSeal> {
    let (compressed, plain_len) = compress(
        input,
        options.level,
        options.dictionary,
        limits.plaintext_bytes(),
    )?;
    let codec = if options.dictionary.is_some() {
        Codec::ZstdDictionary
    } else {
        Codec::Zstd
    };
    let seal = seal_compressed(key, codec, &compressed, out)?;
    Ok(SegmentSeal { plain_len, ..seal })
}

/// Pads and seals an already compressed stream (or, with [`Codec::None`], raw bytes such as a
/// dictionary) and writes the `OXSG` bytes to `out`.
pub fn seal_compressed(
    key: &SegmentKey,
    codec: Codec,
    compressed: &[u8],
    mut out: impl Write,
) -> Result<SegmentSeal> {
    let sealed_len = sealed_segment_len(compressed.len() as u64)?;
    if sealed_len > PACK_BYTES {
        return Err(Error::Limit("segment size"));
    }
    let plaintext = Zeroizing::new(SegmentPlaintext { codec, compressed }.encode()?);

    let salt = random::array()?;
    let keys = key.object_keys(&salt);
    let header = SegmentHeader {
        salt,
        k_commit: keys.commit(),
    }
    .encode();
    let aad = blake3::hash(&header);
    let cipher = Cipher::new(keys.enc())?;

    let mut hasher = blake3::Hasher::new();
    out.write_all(&header)?;
    hasher.update(&header);
    let mut written = header.len() as u64;

    let chunks = plaintext.chunks(CHUNK_PLAINTEXT_LEN);
    let last = chunks.len() - 1;
    let mut buf = Zeroizing::new(Vec::with_capacity(CHUNK_PLAINTEXT_LEN + CHUNK_TAG_LEN));
    for (i, chunk) in chunks.enumerate() {
        buf.clear();
        buf.extend_from_slice(chunk);
        cipher.seal(chunk_nonce(i as u64, i == last), aad.as_bytes(), &mut buf)?;
        out.write_all(&buf)?;
        hasher.update(&buf);
        written += buf.len() as u64;
    }
    out.flush()?;
    debug_assert_eq!(written, sealed_len);
    Ok(SegmentSeal {
        id: SegmentId(hasher.finalize().into()),
        sealed_len: written,
        plain_len: 0,
        compressed_len: compressed.len() as u64,
    })
}

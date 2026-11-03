//! Opening: id, header and commitment checks, STREAM decryption, then bounded decompression.

use std::io::Write;

use oxabl_cache_format::segment::{
    CHUNK_PLAINTEXT_LEN, CHUNK_TAG_LEN, SEGMENT_HEADER_LEN, SealedSegment, plaintext_len,
};
use oxabl_cache_format::{Codec, Limits, SegmentId, SegmentPlaintext};
use zeroize::Zeroizing;

use crate::aead::Cipher;
use crate::compress::decompress;
use crate::dictionary::Dictionary;
use crate::error::{Error, Result};
use crate::keys::SegmentKey;
use crate::nonce::chunk_nonce;

/// What a caller already knows about a segment from its signed record.
pub struct SegmentExpectation<'a> {
    pub id: SegmentId,
    /// Exact length of the sealed bytes.
    pub sealed_len: u64,
    /// Exact length of the decompressed bytes.
    pub plain_len: u64,
    /// The verified dictionary the record names, if any.
    pub dictionary: Option<&'a Dictionary>,
}

/// Decrypts every chunk of `sealed` and returns the `OXSP` plaintext. Authenticates the
/// whole stream before returning, so no byte of an altered segment is released.
pub(crate) fn decrypt_segment(key: &SegmentKey, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let segment = SealedSegment::decode(sealed)?;
    let keys = key.object_keys(&segment.header.salt);
    if !keys.commits_to(&segment.header.k_commit) {
        return Err(Error::KeyCommitment);
    }
    let aad = blake3::hash(&sealed[..SEGMENT_HEADER_LEN]);
    let cipher = Cipher::new(keys.enc())?;
    let total = plaintext_len(segment.body.len())?;
    let mut plaintext = Zeroizing::new(Vec::with_capacity(total as usize));
    let mut scratch = Zeroizing::new(Vec::with_capacity(CHUNK_PLAINTEXT_LEN + CHUNK_TAG_LEN));
    let chunks = segment.body.chunks(CHUNK_PLAINTEXT_LEN + CHUNK_TAG_LEN);
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.enumerate() {
        scratch.clear();
        scratch.extend_from_slice(chunk);
        let opened = cipher.open(
            chunk_nonce(i as u64, i == last),
            aad.as_bytes(),
            &mut scratch,
        )?;
        plaintext.extend_from_slice(opened);
    }
    Ok(plaintext)
}

/// Checks `BLAKE3(sealed)` against the signed segment id.
pub(crate) fn verify_id(sealed: &[u8], id: &SegmentId) -> Result<()> {
    if blake3::hash(sealed).as_bytes() != &id.0 {
        return Err(Error::SegmentId);
    }
    Ok(())
}

/// Verifies and opens an artifact segment, writing the concatenated files to `out`.
///
/// Nothing is written unless every chunk authenticates; the decompressed output is then
/// bounded by the signed `plain_len`. A failure while writing leaves `out` partial, so the
/// caller discards it on any error.
pub fn open_segment(
    key: &SegmentKey,
    sealed: &[u8],
    expect: &SegmentExpectation<'_>,
    limits: &Limits,
    out: impl Write,
) -> Result<()> {
    if expect.plain_len > limits.plaintext_bytes() {
        return Err(Error::Limit("plaintext per artifact set"));
    }
    if sealed.len() as u64 != expect.sealed_len {
        return Err(Error::Mismatch("sealed length"));
    }
    verify_id(sealed, &expect.id)?;
    let plaintext = decrypt_segment(key, sealed)?;
    let body = SegmentPlaintext::decode(&plaintext)?;
    match (body.codec, expect.dictionary) {
        (Codec::Zstd, None) | (Codec::ZstdDictionary, Some(_)) => {}
        _ => return Err(Error::Mismatch("codec and dictionary")),
    }
    decompress(body.compressed, expect.dictionary, expect.plain_len, out)
}

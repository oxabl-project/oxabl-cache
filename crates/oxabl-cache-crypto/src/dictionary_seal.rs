//! A dictionary travels as a segment whose plaintext is the raw dictionary with codec none.

use std::io::Write;

use oxabl_cache_format::{Codec, SegmentId, SegmentPlaintext};

use crate::dictionary::{Dictionary, MAX_DICTIONARY_BYTES};
use crate::error::{Error, Result};
use crate::keys::SegmentKey;
use crate::segment_info::SegmentSeal;
use crate::segment_open::{decrypt_segment, verify_id};
use crate::segment_seal::seal_compressed;

/// Seals a raw dictionary as a dictionary-kind segment.
pub fn seal_dictionary(
    key: &SegmentKey,
    dictionary: &[u8],
    out: impl Write,
) -> Result<SegmentSeal> {
    if dictionary.is_empty() || dictionary.len() > MAX_DICTIONARY_BYTES {
        return Err(Error::Limit("dictionary size"));
    }
    seal_compressed(key, Codec::None, dictionary, out)
}

/// Verifies `sealed` against the signed dictionary id, opens it and prepares the dictionary
/// for compression at `level`.
pub fn open_dictionary(
    key: &SegmentKey,
    sealed: &[u8],
    id: &SegmentId,
    level: i32,
) -> Result<Dictionary> {
    verify_id(sealed, id)?;
    let plaintext = decrypt_segment(key, sealed)?;
    let body = SegmentPlaintext::decode(&plaintext)?;
    if body.codec != Codec::None {
        return Err(Error::Mismatch("not a dictionary segment"));
    }
    Dictionary::new(*id, body.compressed.to_vec(), level)
}

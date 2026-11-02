//! Crockford base32: lowercase, no padding, bytes read as one big-endian integer.

use crate::error::{FormatError, Result};

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Number of characters that spell `bytes` bytes.
pub const fn encoded_len(bytes: usize) -> usize {
    (bytes * 8).div_ceil(5)
}

/// Spells `bytes` as one big-endian integer, padded with leading zero bits to a whole
/// number of characters.
pub fn encode(bytes: &[u8]) -> String {
    let pad = encoded_len(bytes.len()) * 5 - bytes.len() * 8;
    let mut out = String::with_capacity(encoded_len(bytes.len()));
    let (mut acc, mut bits) = (0u32, pad as u32);
    for &b in bytes {
        acc = acc << 8 | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[(acc >> bits & 31) as usize]));
        }
        acc &= (1 << bits) - 1;
    }
    out
}

/// Reads exactly `out.len()` bytes. Rejects uppercase, other characters, a wrong length and
/// set padding bits, so every value has one spelling.
pub fn decode_into(s: &str, out: &mut [u8]) -> Result<()> {
    let chars = s.as_bytes();
    if chars.len() != encoded_len(out.len()) {
        return Err(FormatError::Invalid("base32 length"));
    }
    let pad = (chars.len() * 5 - out.len() * 8) as u32;
    let (mut acc, mut bits, mut written) = (0u32, 0u32, 0usize);
    for (i, &c) in chars.iter().enumerate() {
        let v = ALPHABET
            .iter()
            .position(|&a| a == c)
            .ok_or(FormatError::Invalid("base32 character"))? as u32;
        if i == 0 {
            if v >> (5 - pad) != 0 {
                return Err(FormatError::Invalid("base32 padding"));
            }
            bits = 5 - pad;
            acc = v & ((1 << bits) - 1);
            continue;
        }
        acc = acc << 5 | v;
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            out[written] = (acc >> bits & 0xff) as u8;
            written += 1;
        }
        acc &= (1 << bits) - 1;
    }
    Ok(())
}

/// [`decode_into`] for a fixed length.
pub fn decode<const N: usize>(s: &str) -> Result<[u8; N]> {
    let mut out = [0u8; N];
    decode_into(s, &mut out)?;
    Ok(out)
}

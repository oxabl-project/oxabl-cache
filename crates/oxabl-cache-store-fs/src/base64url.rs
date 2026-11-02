//! Unpadded URL-safe base64, strict on decode.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encodes bytes without padding.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0u32, |acc, (i, byte)| {
            acc | (u32::from(*byte) << (16 - 8 * i))
        });
        for position in 0..=chunk.len() {
            let index = (word >> (18 - 6 * position)) & 63;
            out.push(char::from(ALPHABET[index as usize]));
        }
    }
    out
}

/// Decodes canonical unpadded text; anything else is `None`.
pub(crate) fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.as_bytes().chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut word = 0u32;
        for (i, ch) in chunk.iter().enumerate() {
            let value = ALPHABET.iter().position(|c| c == ch)?;
            word |= u32::try_from(value).ok()? << (18 - 6 * i);
        }
        let bytes = word.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    (encode(&out) == text).then_some(out)
}

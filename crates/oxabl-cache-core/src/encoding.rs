//! Lowercase hex and lowercase Crockford base32, both strict on decode.

const BASE32: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Encodes bytes as lowercase hex.
pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    out
}

/// Decodes exactly `N` bytes from lowercase hex.
pub fn hex_decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.as_bytes();
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    let (pairs, _) = text.as_chunks::<2>();
    for (slot, pair) in out.iter_mut().zip(pairs) {
        *slot = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(out)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Encodes bytes as one big-endian integer in lowercase Crockford base32, no padding.
pub fn base32_encode(bytes: &[u8]) -> String {
    let symbols = (bytes.len() * 8).div_ceil(5);
    let pad = symbols * 5 - bytes.len() * 8;
    (0..symbols)
        .map(|symbol| {
            let value = (0..5).fold(0usize, |acc, offset| {
                (acc << 1) | usize::from(bit(bytes, symbol * 5 + offset, pad))
            });
            char::from(BASE32[value])
        })
        .collect()
}

fn bit(bytes: &[u8], position: usize, pad: usize) -> u8 {
    match position.checked_sub(pad) {
        None => 0,
        Some(index) => (bytes[index / 8] >> (7 - index % 8)) & 1,
    }
}

/// Decodes exactly `N` bytes from the canonical lowercase Crockford base32 form.
pub fn base32_decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    let symbols = (N * 8).div_ceil(5);
    let pad = symbols * 5 - N * 8;
    let text = text.as_bytes();
    if text.len() != symbols {
        return None;
    }
    let mut out = [0u8; N];
    for (symbol, &ch) in text.iter().enumerate() {
        let value = BASE32.iter().position(|&c| c == ch)?;
        for offset in 0..5 {
            let on = (value >> (4 - offset)) & 1 == 1;
            match (symbol * 5 + offset).checked_sub(pad) {
                None if on => return None,
                None => {}
                Some(index) => out[index / 8] |= u8::from(on) << (7 - index % 8),
            }
        }
    }
    Some(out)
}

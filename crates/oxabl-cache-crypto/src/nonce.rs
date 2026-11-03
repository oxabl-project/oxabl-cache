//! STREAM nonces: an 88-bit big-endian chunk counter and a last-chunk flag byte.

pub(crate) const NONCE_LEN: usize = 12;

pub(crate) fn chunk_nonce(index: u64, last: bool) -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    nonce[3..11].copy_from_slice(&index.to_be_bytes());
    nonce[11] = u8::from(last);
    nonce
}

pub(crate) const RECORD_NONCE: [u8; NONCE_LEN] = [0; NONCE_LEN];

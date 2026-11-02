//! Helpers shared by the fuzz targets of `oxabl-cache-format` and `oxabl-cache-crypto`.

pub mod crypto;
pub mod crypto_fixture;
pub mod structured;

/// BLAKE3, the hash the pack trailer commits to.
pub fn blake3(bytes: &[u8]) -> [u8; 32] {
    *::blake3::hash(bytes).as_bytes()
}

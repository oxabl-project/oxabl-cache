use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};

use crate::error::{Error, Result};
use crate::nonce::NONCE_LEN;

/// AES-256-GCM under one single-use key.
pub(crate) struct Cipher(LessSafeKey);

impl Cipher {
    pub(crate) fn new(key: &[u8; 32]) -> Result<Self> {
        let key = UnboundKey::new(&AES_256_GCM, key).map_err(|_| Error::Authentication)?;
        Ok(Self(LessSafeKey::new(key)))
    }

    /// Encrypts `buf` in place and appends the 16-byte tag.
    pub(crate) fn seal(&self, nonce: [u8; NONCE_LEN], aad: &[u8], buf: &mut Vec<u8>) -> Result<()> {
        self.0
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad), buf)
            .map_err(|_| Error::Authentication)
    }

    /// Decrypts `ciphertext_and_tag` in place and returns the plaintext prefix.
    pub(crate) fn open<'a>(
        &self,
        nonce: [u8; NONCE_LEN],
        aad: &[u8],
        ciphertext_and_tag: &'a mut [u8],
    ) -> Result<&'a mut [u8]> {
        self.0
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(aad),
                ciphertext_and_tag,
            )
            .map_err(|_| Error::Authentication)
    }
}

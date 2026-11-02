//! `OXER` sealed record: the outer header.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::VERSION;
use crate::limits::{RECORD_ALIGN, SEALED_RECORD_BYTES};
use crate::writer::Writer;

const MAGIC: [u8; 4] = *b"OXER";
const ALG_AES256GCM: u8 = 1;

/// Length of the record header.
pub const RECORD_HEADER_LEN: usize = 72;
/// Length of the authentication tag that ends a record.
pub const RECORD_TAG_LEN: usize = 16;

/// A sealed record: header fields and the ciphertext with its tag. A valid record is a
/// multiple of 256 bytes long, at most 4,096, and large enough for the smallest `OXEP`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedRecord<'a> {
    pub salt: [u8; 32],
    pub k_commit: [u8; 32],
    /// AES-256-GCM output: ciphertext then the 16-byte tag.
    pub ciphertext: &'a [u8],
}

/// Sealed length of a record whose plaintext is `plaintext_len` bytes before padding.
pub(crate) fn padded_plaintext_len(plaintext_len: usize) -> usize {
    let sealed = RECORD_HEADER_LEN + plaintext_len + RECORD_TAG_LEN;
    sealed.next_multiple_of(RECORD_ALIGN) - RECORD_HEADER_LEN - RECORD_TAG_LEN
}

impl<'a> SealedRecord<'a> {
    fn check_len(total: usize) -> Result<()> {
        if total > SEALED_RECORD_BYTES {
            return Err(FormatError::LimitExceeded("sealed record"));
        }
        if !total.is_multiple_of(RECORD_ALIGN)
            || total < crate::entry_payload::MIN_SEALED_RECORD_LEN
        {
            return Err(FormatError::Invalid("sealed record length"));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        Self::check_len(RECORD_HEADER_LEN + self.ciphertext.len())?;
        let mut w = Writer::with_capacity(RECORD_HEADER_LEN + self.ciphertext.len());
        w.bytes(&MAGIC);
        w.u8(VERSION);
        w.u8(ALG_AES256GCM);
        w.u16(0);
        w.bytes(&self.salt);
        w.bytes(&self.k_commit);
        w.bytes(self.ciphertext);
        Ok(w.into_vec())
    }

    pub fn decode(bytes: &'a [u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        if c.array::<4>()? != MAGIC {
            return Err(FormatError::BadMagic);
        }
        let version = c.u8()?;
        if version != VERSION {
            return Err(FormatError::UnsupportedVersion(version));
        }
        if c.u8()? != ALG_AES256GCM {
            return Err(FormatError::Invalid("record algorithm"));
        }
        c.reserved_u16()?;
        let salt = c.array()?;
        let k_commit = c.array()?;
        Self::check_len(bytes.len())?;
        let ciphertext = c.take(c.remaining())?;
        Ok(Self {
            salt,
            k_commit,
            ciphertext,
        })
    }
}

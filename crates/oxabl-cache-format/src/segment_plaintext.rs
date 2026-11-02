//! `OXSP` plaintext of a segment, after decryption.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::VERSION;
use crate::padme::padme;
use crate::writer::Writer;

const MAGIC: [u8; 4] = *b"OXSP";
const HEADER_LEN: usize = 14;

/// How the compressed bytes were produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    None,
    Zstd,
    ZstdDictionary,
}

impl Codec {
    fn to_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Zstd => 1,
            Self::ZstdDictionary => 2,
        }
    }

    fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::None),
            1 => Ok(Self::Zstd),
            2 => Ok(Self::ZstdDictionary),
            _ => Err(FormatError::Invalid("codec")),
        }
    }
}

/// The plaintext of a segment: a codec and the compressed stream, then zero padding to the
/// Padmé length of everything before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentPlaintext<'a> {
    pub codec: Codec,
    pub compressed: &'a [u8],
}

impl<'a> SegmentPlaintext<'a> {
    fn padded_len(compressed_len: usize) -> Result<usize> {
        let unpadded = (HEADER_LEN as u64)
            .checked_add(compressed_len as u64)
            .ok_or(FormatError::Invalid("plaintext length"))?;
        padme(unpadded)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(FormatError::Invalid("plaintext length"))
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let padded = Self::padded_len(self.compressed.len())?;
        let mut w = Writer::with_capacity(padded);
        w.bytes(&MAGIC);
        w.u8(VERSION);
        w.u8(self.codec.to_u8());
        w.u64(self.compressed.len() as u64);
        w.bytes(self.compressed);
        w.zeros(padded - w.len());
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
        let codec = Codec::from_u8(c.u8()?)?;
        let len = c.u64()?;
        let len = usize::try_from(len)
            .ok()
            .filter(|&n| n <= c.remaining())
            .ok_or(FormatError::Truncated)?;
        let compressed = c.take(len)?;
        if bytes.len() != Self::padded_len(len)? {
            return Err(FormatError::Invalid("padding length"));
        }
        let padding = c.take(c.remaining())?;
        if padding.iter().any(|&b| b != 0) {
            return Err(FormatError::Invalid("padding is not zero"));
        }
        Ok(Self { codec, compressed })
    }
}

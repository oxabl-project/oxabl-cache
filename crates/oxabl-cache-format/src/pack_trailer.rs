//! `OXPK` pack trailer.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::limits::{PACK_BYTES, PACK_SEGMENTS};
use crate::pack_footer::FOOTER_ENTRY_LEN;
use crate::pack_header::PACK_HEADER_LEN;
use crate::writer::Writer;

const MAGIC: [u8; 4] = *b"OXPF";

/// Length of the trailer, the last bytes of a pack.
pub const PACK_TRAILER_LEN: usize = 48;

/// The trailer of a pack object: where the footer is and what it must hash to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackTrailer {
    pub segment_count: u32,
    pub footer_offset: u64,
    /// BLAKE3 of the footer.
    pub footer_digest: [u8; 32],
}

impl PackTrailer {
    /// Size of the whole pack object this trailer describes.
    pub fn object_size(&self) -> Result<u64> {
        u64::from(self.segment_count)
            .checked_mul(FOOTER_ENTRY_LEN as u64)
            .and_then(|footer| footer.checked_add(self.footer_offset))
            .and_then(|end| end.checked_add(PACK_TRAILER_LEN as u64))
            .ok_or(FormatError::Invalid("pack size"))
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.segment_count as usize > PACK_SEGMENTS {
            return Err(FormatError::LimitExceeded("segments"));
        }
        if self.footer_offset < PACK_HEADER_LEN as u64 {
            return Err(FormatError::Invalid("footer offset"));
        }
        if self.object_size()? > PACK_BYTES {
            return Err(FormatError::LimitExceeded("pack size"));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<[u8; PACK_TRAILER_LEN]> {
        self.check()?;
        let mut w = Writer::with_capacity(PACK_TRAILER_LEN);
        w.u32(self.segment_count);
        w.u64(self.footer_offset);
        w.bytes(&self.footer_digest);
        w.bytes(&MAGIC);
        let mut out = [0u8; PACK_TRAILER_LEN];
        out.copy_from_slice(&w.into_vec());
        Ok(out)
    }

    /// Decodes the last 48 bytes of a pack of `object_size` bytes and checks they agree
    /// with that size.
    pub fn decode(bytes: &[u8], object_size: u64) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        let segment_count = c.u32()?;
        let footer_offset = c.u64()?;
        let footer_digest = c.array()?;
        if c.array::<4>()? != MAGIC {
            return Err(FormatError::BadMagic);
        }
        c.finish()?;
        let trailer = Self {
            segment_count,
            footer_offset,
            footer_digest,
        };
        trailer.check()?;
        if trailer.object_size()? != object_size {
            return Err(FormatError::Invalid("trailer disagrees with the pack size"));
        }
        Ok(trailer)
    }
}

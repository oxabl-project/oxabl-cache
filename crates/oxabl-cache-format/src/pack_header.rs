//! `OXPK` pack header.

use crate::cursor::Cursor;
use crate::error::Result;
use crate::frame::{read_header, write_header};
use crate::ids::CacheId;
use crate::writer::Writer;

const MAGIC: [u8; 4] = *b"OXPK";

/// Length of the pack header; the first segment starts here.
pub const PACK_HEADER_LEN: usize = 24;

/// The header of a pack object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackHeader {
    pub cache_id: CacheId,
}

impl PackHeader {
    pub fn encode(&self) -> [u8; PACK_HEADER_LEN] {
        let mut w = Writer::with_capacity(PACK_HEADER_LEN);
        write_header(&mut w, MAGIC, 0);
        w.u16(0);
        w.bytes(&self.cache_id.0);
        let mut out = [0u8; PACK_HEADER_LEN];
        out.copy_from_slice(&w.into_vec());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        read_header(&mut c, MAGIC, 0)?;
        c.reserved_u16()?;
        let cache_id = CacheId(c.array()?);
        c.finish()?;
        Ok(Self { cache_id })
    }
}

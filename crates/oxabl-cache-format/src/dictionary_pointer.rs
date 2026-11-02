//! `OXDP` dictionary pointer.

use crate::cursor::Cursor;
use crate::error::Result;
use crate::frame::{read_header, write_header};
use crate::ids::{CacheId, PublicKey, SegmentId, Signature};
use crate::writer::Writer;

const MAGIC: [u8; 4] = *b"OXDP";

/// Length of an encoded pointer.
pub const DICTIONARY_POINTER_LEN: usize = 6 + 16 + 4 + 8 + 32 + 32 + 64;

/// Names the dictionary segment of a cache, signed by a `ci` or `owner` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictionaryPointer {
    pub cache_id: CacheId,
    pub epoch: u32,
    pub version: u64,
    pub dict_segment_id: SegmentId,
    pub signer: PublicKey,
    pub signature: Signature,
}

impl DictionaryPointer {
    /// Every byte before the signature: what it covers, after the caller's domain-separated
    /// prefix.
    pub fn signed_bytes(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(DICTIONARY_POINTER_LEN);
        write_header(&mut w, MAGIC, 0);
        w.bytes(&self.cache_id.0);
        w.u32(self.epoch);
        w.u64(self.version);
        w.bytes(&self.dict_segment_id.0);
        w.bytes(&self.signer);
        w.into_vec()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.signed_bytes();
        out.extend_from_slice(&self.signature);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        read_header(&mut c, MAGIC, 0)?;
        let pointer = Self {
            cache_id: CacheId(c.array()?),
            epoch: c.u32()?,
            version: c.u64()?,
            dict_segment_id: SegmentId(c.array()?),
            signer: c.array()?,
            signature: c.array()?,
        };
        c.finish()?;
        Ok(pointer)
    }
}

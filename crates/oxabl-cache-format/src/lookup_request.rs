//! `OXLQ` lookup request.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::{read_header, write_header};
use crate::ids::LookupId;
use crate::limits::LOOKUP_IDS;
use crate::namespace::Namespaces;
use crate::writer::{Writer, len32};

const MAGIC: [u8; 4] = *b"OXLQ";

/// Flag bits shared by `OXLQ` and `OXLR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookupFlags {
    /// Return sealed records, not presence only.
    pub records: bool,
    /// Return presigned GET URLs.
    pub urls: bool,
}

impl LookupFlags {
    pub(crate) fn to_bits(self) -> u8 {
        u8::from(self.records) | u8::from(self.urls) << 1
    }

    pub(crate) fn from_bits(bits: u8) -> Self {
        Self {
            records: bits & 1 != 0,
            urls: bits & 2 != 0,
        }
    }
}

/// What a response must agree with: the request's flags, namespaces and id count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookupShape {
    pub flags: LookupFlags,
    pub namespaces: Namespaces,
    pub count: u32,
}

/// A lookup request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupRequest {
    pub flags: LookupFlags,
    pub namespaces: Namespaces,
    pub ids: Vec<LookupId>,
}

impl LookupRequest {
    /// The shape a response to this request must have.
    pub fn shape(&self) -> Result<LookupShape> {
        Ok(LookupShape {
            flags: self.flags,
            namespaces: self.namespaces,
            count: len32(self.ids.len(), "lookup ids")?,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.ids.len() > LOOKUP_IDS {
            return Err(FormatError::LimitExceeded("lookup ids"));
        }
        if self.namespaces.is_empty() {
            return Err(FormatError::Invalid("namespaces"));
        }
        let mut w = Writer::with_capacity(12 + self.ids.len() * LookupId::LEN);
        write_header(&mut w, MAGIC, self.flags.to_bits());
        w.u8(self.namespaces.to_bits());
        w.u8(0);
        w.u32(len32(self.ids.len(), "lookup ids")?);
        for id in &self.ids {
            w.bytes(&id.0);
        }
        Ok(w.into_vec())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        let flags = LookupFlags::from_bits(read_header(&mut c, MAGIC, 0b11)?);
        let namespaces = Namespaces::from_bits(c.u8()?)?;
        c.reserved_u8()?;
        let count = c.count32(LOOKUP_IDS, "lookup ids", LookupId::LEN)?;
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            ids.push(LookupId(c.array()?));
        }
        c.finish()?;
        Ok(Self {
            flags,
            namespaces,
            ids,
        })
    }
}

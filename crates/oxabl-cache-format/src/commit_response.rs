//! `OXCA` commit response.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::{read_header, write_header};
use crate::limits::COMMIT_ENTRIES;
use crate::writer::{Writer, len32};

const MAGIC: [u8; 4] = *b"OXCA";

/// Outcome of one entry of a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitStatus {
    Committed,
    Exists,
    Invalid,
    MissingDictionary,
}

impl CommitStatus {
    fn to_u8(self) -> u8 {
        match self {
            Self::Committed => 0,
            Self::Exists => 1,
            Self::Invalid => 2,
            Self::MissingDictionary => 3,
        }
    }

    fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::Committed),
            1 => Ok(Self::Exists),
            2 => Ok(Self::Invalid),
            3 => Ok(Self::MissingDictionary),
            _ => Err(FormatError::Invalid("commit status")),
        }
    }
}

/// A commit response: one status per entry, in request order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResponse {
    pub statuses: Vec<CommitStatus>,
}

impl CommitResponse {
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.statuses.len() > COMMIT_ENTRIES {
            return Err(FormatError::LimitExceeded("entries"));
        }
        let mut w = Writer::with_capacity(12 + self.statuses.len());
        write_header(&mut w, MAGIC, 0);
        w.u16(0);
        w.u32(len32(self.statuses.len(), "entries")?);
        for s in &self.statuses {
            w.u8(s.to_u8());
        }
        Ok(w.into_vec())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        read_header(&mut c, MAGIC, 0)?;
        c.reserved_u16()?;
        let count = c.count32(COMMIT_ENTRIES, "entries", 1)?;
        let mut statuses = Vec::with_capacity(count);
        for _ in 0..count {
            statuses.push(CommitStatus::from_u8(c.u8()?)?);
        }
        c.finish()?;
        Ok(Self { statuses })
    }
}

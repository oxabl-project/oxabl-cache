use crate::error::{FormatError, Result};
use crate::limits::SEALED_RECORD_BYTES;

/// Big-endian byte writer.
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u16(&mut self, v: u16) {
        self.bytes(&v.to_be_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_be_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_be_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.bytes(&v.to_be_bytes());
    }

    pub fn zeros(&mut self, n: usize) {
        self.buf.resize(self.buf.len() + n, 0);
    }

    pub fn str16(&mut self, s: &str) -> Result<()> {
        let len = u16::try_from(s.len()).map_err(|_| FormatError::LimitExceeded("str16 length"))?;
        self.u16(len);
        self.bytes(s.as_bytes());
        Ok(())
    }

    /// Writes a `u16` length and the record, at most one sealed record.
    pub fn record(&mut self, record: &[u8]) -> Result<()> {
        if record.len() > SEALED_RECORD_BYTES {
            return Err(FormatError::LimitExceeded("sealed record"));
        }
        self.u16(record.len() as u16);
        self.bytes(record);
        Ok(())
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}

/// Converts a length to `u32`, failing when it does not fit.
pub fn len32(len: usize, what: &'static str) -> Result<u32> {
    u32::try_from(len).map_err(|_| FormatError::LimitExceeded(what))
}

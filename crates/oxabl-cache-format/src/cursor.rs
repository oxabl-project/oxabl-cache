use crate::error::{FormatError, Result};
use crate::limits::SEALED_RECORD_BYTES;

/// Bounded big-endian reader over untrusted bytes. Every read checks the remaining length.
pub struct Cursor<'a> {
    buf: &'a [u8],
}

impl<'a> Cursor<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len()
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.buf.len() {
            return Err(FormatError::Truncated);
        }
        let (head, tail) = self.buf.split_at(n);
        self.buf = tail;
        Ok(head)
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.array()?))
    }

    pub fn str16(&mut self) -> Result<&'a str> {
        let len = usize::from(self.u16()?);
        std::str::from_utf8(self.take(len)?).map_err(|_| FormatError::InvalidUtf8)
    }

    /// Reads a `u16` length and that many bytes, at most one sealed record.
    pub fn record(&mut self) -> Result<&'a [u8]> {
        let len = usize::from(self.u16()?);
        if len > SEALED_RECORD_BYTES {
            return Err(FormatError::LimitExceeded("sealed record"));
        }
        self.take(len)
    }

    pub fn reserved_u8(&mut self) -> Result<()> {
        match self.u8()? {
            0 => Ok(()),
            _ => Err(FormatError::NonZeroReserved),
        }
    }

    pub fn reserved_u16(&mut self) -> Result<()> {
        match self.u16()? {
            0 => Ok(()),
            _ => Err(FormatError::NonZeroReserved),
        }
    }

    /// Checks a declared element count against `limit` and against the bytes left, so a
    /// count can never drive an allocation larger than the input.
    pub fn check_count(
        &self,
        count: u64,
        limit: usize,
        what: &'static str,
        min_element: usize,
    ) -> Result<usize> {
        let count = usize::try_from(count).map_err(|_| FormatError::LimitExceeded(what))?;
        if count > limit {
            return Err(FormatError::LimitExceeded(what));
        }
        match count.checked_mul(min_element) {
            Some(need) if need <= self.buf.len() => Ok(count),
            _ => Err(FormatError::Truncated),
        }
    }

    pub fn count32(
        &mut self,
        limit: usize,
        what: &'static str,
        min_element: usize,
    ) -> Result<usize> {
        let n = self.u32()?;
        self.check_count(u64::from(n), limit, what, min_element)
    }

    pub fn count16(
        &mut self,
        limit: usize,
        what: &'static str,
        min_element: usize,
    ) -> Result<usize> {
        let n = self.u16()?;
        self.check_count(u64::from(n), limit, what, min_element)
    }

    /// Succeeds only when every byte has been read.
    pub fn finish(self) -> Result<()> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(FormatError::TrailingBytes)
        }
    }
}

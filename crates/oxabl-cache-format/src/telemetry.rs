//! `OXTL` sealed telemetry report and the report it carries.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::{read_header, write_header};
use crate::limits::TELEMETRY_BYTES;
use crate::writer::Writer;

const MAGIC: [u8; 4] = *b"OXTL";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Bytes before the ciphertext: magic, version, flags, epoch and nonce.
pub const TELEMETRY_HEADER_LEN: usize = 6 + 4 + NONCE_LEN;
/// Length of an encoded [`TelemetryReport`].
pub const TELEMETRY_REPORT_LEN: usize = 16 + 8 + 1 + 1 + 4 * 4 + 8 + 4 + 4;

/// The outer shape of a sealed report: what the server may check without a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedTelemetry<'a> {
    pub epoch: u32,
    pub nonce: [u8; NONCE_LEN],
    /// Ciphertext followed by the 16-byte tag.
    pub sealed: &'a [u8],
}

impl<'a> SealedTelemetry<'a> {
    /// The header the sealed bytes are authenticated with, as written on the wire.
    pub fn header(epoch: u32, nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
        let mut w = Writer::with_capacity(TELEMETRY_HEADER_LEN);
        write_header(&mut w, MAGIC, 0);
        w.u32(epoch);
        w.bytes(nonce);
        w.into_vec()
    }

    pub fn decode(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > TELEMETRY_BYTES {
            return Err(FormatError::LimitExceeded("telemetry report"));
        }
        let mut c = Cursor::new(bytes);
        read_header(&mut c, MAGIC, 0)?;
        let epoch = c.u32()?;
        let nonce = c.array()?;
        let sealed = c.take(c.remaining())?;
        if sealed.len() <= TAG_LEN {
            return Err(FormatError::Truncated);
        }
        Ok(Self {
            epoch,
            nonce,
            sealed,
        })
    }
}

/// What a report measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TelemetryKind {
    /// A fetch: keys asked for, entries installed.
    Pull,
    /// A store: sets put, sets committed or already present.
    Push,
}

/// How the reporting client ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TelemetryMode {
    Ci,
    Developer,
    ReadOnly,
}

/// One fetch or store as its client measured it. Holds no identity, key or path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelemetryReport {
    /// Random; lets a reader drop a report sent twice.
    pub id: [u8; 16],
    /// Unix seconds on the client's clock when the operation ended.
    pub at: u64,
    pub kind: TelemetryKind,
    pub mode: TelemetryMode,
    /// Keys asked for, or sets put.
    pub requested: u32,
    /// Entries installed, or sets committed or already present.
    pub hits: u32,
    /// Keys not in the cache, or sets the cache refused for quota.
    pub misses: u32,
    /// Everything else that did not succeed.
    pub failed: u32,
    /// Bytes received, or bytes uploaded.
    pub bytes: u64,
    /// Wall time of the whole operation.
    pub duration_ms: u32,
    /// Round trip of one small call to the server, measured by the same handle.
    pub rtt_ms: u32,
}

impl TelemetryKind {
    fn code(self) -> u8 {
        match self {
            Self::Pull => 1,
            Self::Push => 2,
        }
    }

    fn from_code(code: u8) -> Result<Self> {
        match code {
            1 => Ok(Self::Pull),
            2 => Ok(Self::Push),
            _ => Err(FormatError::Invalid("telemetry kind")),
        }
    }
}

impl TelemetryMode {
    fn code(self) -> u8 {
        match self {
            Self::Ci => 1,
            Self::Developer => 2,
            Self::ReadOnly => 3,
        }
    }

    fn from_code(code: u8) -> Result<Self> {
        match code {
            1 => Ok(Self::Ci),
            2 => Ok(Self::Developer),
            3 => Ok(Self::ReadOnly),
            _ => Err(FormatError::Invalid("telemetry mode")),
        }
    }
}

impl TelemetryReport {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(TELEMETRY_REPORT_LEN);
        w.bytes(&self.id);
        w.u64(self.at);
        w.u8(self.kind.code());
        w.u8(self.mode.code());
        w.u32(self.requested);
        w.u32(self.hits);
        w.u32(self.misses);
        w.u32(self.failed);
        w.u64(self.bytes);
        w.u32(self.duration_ms);
        w.u32(self.rtt_ms);
        w.into_vec()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        let report = Self {
            id: c.array()?,
            at: c.u64()?,
            kind: TelemetryKind::from_code(c.u8()?)?,
            mode: TelemetryMode::from_code(c.u8()?)?,
            requested: c.u32()?,
            hits: c.u32()?,
            misses: c.u32()?,
            failed: c.u32()?,
            bytes: c.u64()?,
            duration_ms: c.u32()?,
            rtt_ms: c.u32()?,
        };
        c.finish()?;
        if u64::from(report.hits) + u64::from(report.misses) + u64::from(report.failed)
            > u64::from(report.requested)
        {
            return Err(FormatError::Invalid("telemetry counts exceed the request"));
        }
        Ok(report)
    }
}

use std::fmt;

/// Why a byte string is not a valid frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormatError {
    /// The magic bytes differ from the expected frame.
    BadMagic,
    /// The version byte is not supported.
    UnsupportedVersion(u8),
    /// The flags byte has bits that are not defined for the frame.
    UnknownFlags(u8),
    /// A reserved field is not zero.
    NonZeroReserved,
    /// The input ends before the frame does.
    Truncated,
    /// Bytes follow the end of the frame.
    TrailingBytes,
    /// A count or length exceeds the named limit.
    LimitExceeded(&'static str),
    /// A string is not valid UTF-8.
    InvalidUtf8,
    /// The bytes decode but are not the canonical encoding, or break a rule of the format.
    Invalid(&'static str),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => f.write_str("wrong magic"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version {v}"),
            Self::UnknownFlags(v) => write!(f, "unknown flag bits {v:#04x}"),
            Self::NonZeroReserved => f.write_str("reserved field is not zero"),
            Self::Truncated => f.write_str("truncated input"),
            Self::TrailingBytes => f.write_str("trailing bytes"),
            Self::LimitExceeded(what) => write!(f, "limit exceeded: {what}"),
            Self::InvalidUtf8 => f.write_str("string is not valid UTF-8"),
            Self::Invalid(what) => write!(f, "invalid: {what}"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Result of every encode and decode in this crate.
pub type Result<T> = std::result::Result<T, FormatError>;

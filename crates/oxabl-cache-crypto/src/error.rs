use std::fmt;
use std::io;

use oxabl_cache_format::FormatError;

/// Why a cryptographic operation failed. Variants never carry secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not a valid frame of the format.
    Format(FormatError),
    /// The operating system's random generator failed.
    Random,
    /// The key commitment in the header differs from the one derived from the key.
    KeyCommitment,
    /// An authentication tag did not verify: the bytes were altered, truncated or reordered,
    /// or the key is wrong.
    Authentication,
    /// The BLAKE3 of the sealed segment differs from the expected segment id.
    SegmentId,
    /// An Ed25519 signature or public key did not verify.
    Signature,
    /// The compressed stream is invalid or does not meet the rules of the format.
    Compression(&'static str),
    /// A size or count exceeds a limit.
    Limit(&'static str),
    /// A reader or writer failed.
    Io(io::ErrorKind),
    /// A record's signer is not accepted for its namespace under the current roster.
    SignerNotAccepted,
    /// A record names a roster version newer than the current roster.
    RosterVersionAhead,
    /// A record does not belong with the segment or dictionary it is opened against.
    Mismatch(&'static str),
    /// A roster failed verification or is malformed.
    Roster(&'static str),
    /// The identity is not a member of the roster.
    IdentityNotInRoster,
    /// The keyring does not unwrap to a root key for this roster.
    Keyring(&'static str),
    /// An identity string or file is invalid or could not be unlocked.
    Identity(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(e) => write!(f, "format: {e}"),
            Self::Random => f.write_str("random generator failed"),
            Self::KeyCommitment => f.write_str("key commitment mismatch"),
            Self::Authentication => f.write_str("authentication failed"),
            Self::SegmentId => f.write_str("segment id mismatch"),
            Self::Signature => f.write_str("signature invalid"),
            Self::Compression(what) => write!(f, "compression: {what}"),
            Self::Limit(what) => write!(f, "limit exceeded: {what}"),
            Self::Io(kind) => write!(f, "i/o: {kind}"),
            Self::SignerNotAccepted => f.write_str("signer not accepted"),
            Self::RosterVersionAhead => f.write_str("record is ahead of the roster"),
            Self::Mismatch(what) => write!(f, "mismatch: {what}"),
            Self::Roster(what) => write!(f, "roster: {what}"),
            Self::IdentityNotInRoster => f.write_str("identity is not in the roster"),
            Self::Keyring(what) => write!(f, "keyring: {what}"),
            Self::Identity(what) => write!(f, "identity: {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<FormatError> for Error {
    fn from(e: FormatError) -> Self {
        Self::Format(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind())
    }
}

/// Result of every operation in this crate.
pub type Result<T> = std::result::Result<T, Error>;

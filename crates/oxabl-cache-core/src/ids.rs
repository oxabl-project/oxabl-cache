//! Fixed-size identifiers and their text forms.

use crate::encoding::{base32_decode, base32_encode, hex_decode, hex_encode};
use std::fmt;
use std::str::FromStr;

/// An identifier text that does not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseIdError;

impl fmt::Display for ParseIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("malformed identifier")
    }
}

impl std::error::Error for ParseIdError {}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    loop {
        getrandom::fill(&mut bytes).expect("the operating system provides randomness");
        if bytes.iter().any(|b| *b != 0) {
            return bytes;
        }
    }
}

macro_rules! byte_id {
    ($(#[$meta:meta])* $name:ident, $len:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; $len]);

        impl $name {
            /// Length in bytes.
            pub const LEN: usize = $len;

            /// Wraps raw bytes.
            pub const fn from_bytes(bytes: [u8; $len]) -> Self {
                Self(bytes)
            }

            /// The raw bytes.
            pub const fn as_bytes(&self) -> &[u8; $len] {
                &self.0
            }

            /// Reads an id from a slice of exactly the right length.
            pub fn from_slice(bytes: &[u8]) -> Option<Self> {
                bytes.try_into().ok().map(Self)
            }

            /// A fresh random non-zero id.
            pub fn random() -> Self {
                Self(random_bytes())
            }
        }
    };
}

macro_rules! hex_id {
    ($(#[$meta:meta])* $name:ident, $len:literal) => {
        byte_id!($(#[$meta])* $name, $len);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&hex_encode(&self.0))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), hex_encode(&self.0))
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(text: &str) -> Result<Self, ParseIdError> {
                hex_decode(text).map(Self).ok_or(ParseIdError)
            }
        }
    };
}

macro_rules! prefixed_id {
    ($(#[$meta:meta])* $name:ident, $len:literal, $prefix:literal) => {
        byte_id!($(#[$meta])* $name, $len);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $prefix, base32_encode(&self.0))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self)
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(text: &str) -> Result<Self, ParseIdError> {
                let body = text.strip_prefix($prefix).ok_or(ParseIdError)?;
                base32_decode(body).map(Self).ok_or(ParseIdError)
            }
        }
    };
}

prefixed_id!(
    /// A customer account, `t_` plus 26 base32 characters.
    TenantId, 16, "t_"
);
prefixed_id!(
    /// A cache, `c_` plus 26 base32 characters.
    CacheId, 16, "c_"
);
prefixed_id!(
    /// A user, a workload or a token owner, `p_` plus 26 base32 characters.
    PrincipalId, 16, "p_"
);
hex_id!(
    /// A pack, 32 hex characters.
    PackId, 16
);
hex_id!(
    /// A segment, the BLAKE3 hash of its sealed bytes, 64 hex characters.
    SegmentId, 32
);
hex_id!(
    /// A keyed lookup id, 64 hex characters.
    LookupId, 32
);
hex_id!(
    /// A client-chosen retry key for one pack reservation.
    ClientRef, 16
);
hex_id!(
    /// A server process, drawn at each start.
    NodeId, 16
);

byte_id!(
    /// The public id of a token or session, 16 base32 characters.
    CredentialId, 10
);

impl fmt::Display for CredentialId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&base32_encode(&self.0))
    }
}

impl fmt::Debug for CredentialId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CredentialId({self})")
    }
}

impl FromStr for CredentialId {
    type Err = ParseIdError;

    fn from_str(text: &str) -> Result<Self, ParseIdError> {
        base32_decode(text).map(Self).ok_or(ParseIdError)
    }
}

impl SegmentId {
    /// The id a segment with these sealed bytes carries.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }
}

//! Fixed-length identifiers and their lowercase hex form.

use std::fmt;

use crate::error::{FormatError, Result};

/// Ed25519 public key.
pub type PublicKey = [u8; 32];
/// Ed25519 signature.
pub type Signature = [u8; 64];

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident, $len:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub [u8; $len]);

        impl $name {
            /// Length in bytes.
            pub const LEN: usize = $len;

            /// Lowercase hex, `2 * LEN` characters.
            pub fn to_hex(&self) -> String {
                to_hex(&self.0)
            }

            /// Parses lowercase hex of exactly `2 * LEN` characters.
            pub fn from_hex(s: &str) -> Result<Self> {
                let mut out = [0u8; $len];
                from_hex(s, &mut out)?;
                Ok(Self(out))
            }
        }

        impl From<[u8; $len]> for $name {
            fn from(bytes: [u8; $len]) -> Self {
                Self(bytes)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.to_hex())
            }
        }
    };
}

id_type!(
    /// Keyed lookup id of a compile key.
    LookupId,
    32
);
id_type!(
    /// BLAKE3 of a sealed segment; also names a dictionary segment.
    SegmentId,
    32
);
id_type!(
    /// Server-assigned pack id.
    PackId,
    16
);
id_type!(
    /// Cache id as raw bytes.
    CacheId,
    16
);
id_type!(
    /// Tenant id as raw bytes.
    TenantId,
    16
);
id_type!(
    /// Request id as raw bytes.
    RequestId,
    16
);

const HEX: &[u8; 16] = b"0123456789abcdef";

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 15)]));
    }
    out
}

fn nibble(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(FormatError::Invalid("hex digit")),
    }
}

fn from_hex(s: &str, out: &mut [u8]) -> Result<()> {
    let s = s.as_bytes();
    if s.len() != out.len() * 2 {
        return Err(FormatError::Invalid("hex length"));
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = nibble(s[2 * i])? << 4 | nibble(s[2 * i + 1])?;
    }
    Ok(())
}

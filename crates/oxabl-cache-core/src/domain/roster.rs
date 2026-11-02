//! Rosters, owner keys and the dictionary pointer.

use crate::ids::SegmentId;

/// A signing public key.
pub type PublicKey = [u8; 32];
/// An Ed25519 signature.
pub type SignatureBytes = [u8; 64];

/// What a roster member may do, as written in the roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberRole {
    /// Manages the roster.
    Owner,
    /// Produces trusted entries.
    Ci,
    /// Reads trusted entries and writes personal ones.
    Developer,
    /// Reads only.
    Reader,
}

impl MemberRole {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            MemberRole::Owner => 0,
            MemberRole::Ci => 1,
            MemberRole::Developer => 2,
            MemberRole::Reader => 3,
        }
    }

    /// Reads a stored code.
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(MemberRole::Owner),
            1 => Some(MemberRole::Ci),
            2 => Some(MemberRole::Developer),
            3 => Some(MemberRole::Reader),
            _ => None,
        }
    }
}

/// The signing keys of one roster member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterMember {
    /// The member's role.
    pub role: MemberRole,
    /// The member's signing public keys.
    pub sign_keys: Vec<PublicKey>,
}

/// A signature over a signed document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature {
    /// The signer's public key.
    pub key: PublicKey,
    /// The Ed25519 signature.
    pub signature: SignatureBytes,
}

/// A roster as parsed by the frame layer. The keyring stays inside `bytes`, opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    /// Roster version.
    pub version: u64,
    /// Generation of the cache root key.
    pub epoch: u32,
    /// The full encoded roster.
    pub bytes: Vec<u8>,
    /// The members and their public keys.
    pub members: Vec<RosterMember>,
    /// The exact bytes every signature covers.
    pub signed_message: Vec<u8>,
    /// The signatures, one to four.
    pub signatures: Vec<Signature>,
}

/// The stored current roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRoster {
    /// Roster version.
    pub version: u64,
    /// Generation of the cache root key.
    pub epoch: u32,
    /// The full encoded roster.
    pub bytes: Vec<u8>,
}

/// A dictionary pointer as parsed by the frame layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryPointer {
    /// Pointer version.
    pub version: u64,
    /// Generation of the cache root key.
    pub epoch: u32,
    /// The dictionary segment it names.
    pub segment_id: SegmentId,
    /// The full encoded pointer.
    pub bytes: Vec<u8>,
    /// The exact bytes the signature covers.
    pub signed_message: Vec<u8>,
    /// The signer's key and signature.
    pub signature: Signature,
}

/// The stored dictionary pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDictionaryPointer {
    /// Pointer version.
    pub version: u64,
    /// Generation of the cache root key.
    pub epoch: u32,
    /// The dictionary segment it names.
    pub segment_id: SegmentId,
    /// The full encoded pointer.
    pub bytes: Vec<u8>,
}

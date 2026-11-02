//! Verifying a roster against the pins in the customer's repository.

use oxabl_cache_format::{CacheId, PublicKey, Role, Roster};

use crate::domain::{SIG_ROSTER, prefixed};
use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::keyring;
use crate::keys::RootKey;
use crate::roster_member::{MemberEntry, check_members};
use crate::signing;

/// What the repository pins: the cache, its owner keys and the oldest acceptable roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterPins {
    pub cache_id: CacheId,
    pub owner_keys: Vec<PublicKey>,
    pub min_version: u64,
}

/// A roster that a pinned owner signed, at a version that does not roll back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRoster {
    bytes: Vec<u8>,
    cache_id: CacheId,
    version: u64,
    epoch: u32,
    members: Vec<MemberEntry>,
    keyring: Vec<u8>,
    signers: Vec<PublicKey>,
}

impl VerifiedRoster {
    /// Accepts `bytes` only if the cache matches, the version is at least `max(min_version,
    /// high_water)`, at least one valid signature is by a pinned owner key, and the members
    /// are well formed. Unpinned signatures are ignored.
    pub fn verify(bytes: &[u8], pins: &RosterPins, high_water: u64) -> Result<Self> {
        let roster = Roster::decode(bytes)?;
        if roster.cache_id != pins.cache_id {
            return Err(Error::Roster("another cache"));
        }
        if roster.version < pins.min_version.max(high_water) {
            return Err(Error::Roster(
                "version is below the pinned minimum or the high-water mark",
            ));
        }
        if roster.version == 0 || roster.epoch == 0 {
            return Err(Error::Roster("version or epoch is zero"));
        }
        let transcript = prefixed(SIG_ROSTER, &roster.signed_bytes()?);
        let mut signers = Vec::new();
        for s in &roster.signatures {
            if pins.owner_keys.contains(&s.owner_key)
                && !signers.contains(&s.owner_key)
                && signing::verify(&s.owner_key, &transcript, &s.signature).is_ok()
            {
                signers.push(s.owner_key);
            }
        }
        if signers.is_empty() {
            return Err(Error::Roster("no valid signature by a pinned owner key"));
        }
        let members: Vec<MemberEntry> = roster.members.iter().map(MemberEntry::from_wire).collect();
        check_members(&members)?;
        Ok(Self {
            bytes: bytes.to_vec(),
            cache_id: roster.cache_id,
            version: roster.version,
            epoch: roster.epoch,
            members,
            keyring: roster.keyring.to_vec(),
            signers,
        })
    }

    pub fn cache_id(&self) -> CacheId {
        self.cache_id
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    pub fn members(&self) -> &[MemberEntry] {
        &self.members
    }

    /// The pinned owner keys whose signatures verified.
    pub fn signers(&self) -> &[PublicKey] {
        &self.signers
    }

    /// The verified `OXRS` bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The member that lists `key` as a signing key.
    pub fn member_by_sign_key(&self, key: &PublicKey) -> Option<&MemberEntry> {
        self.members.iter().find(|m| m.has_sign_key(key))
    }

    /// Whether `key` is a signing key of a `ci` member.
    pub fn is_ci_key(&self, key: &PublicKey) -> bool {
        self.member_by_sign_key(key)
            .is_some_and(|m| m.role == Role::Ci)
    }

    /// Whether `key` is a signing key of a `ci` or `owner` member.
    pub fn is_ci_or_owner_key(&self, key: &PublicKey) -> bool {
        self.member_by_sign_key(key)
            .is_some_and(|m| matches!(m.role, Role::Ci | Role::Owner))
    }

    /// The member entry of `identity`, which must list both its signing key and its recipient.
    pub fn member_of(&self, identity: &Identity) -> Result<&MemberEntry> {
        let recipient = identity.recipient();
        self.member_by_sign_key(&identity.signing_public())
            .filter(|m| m.age_recipients.contains(&recipient))
            .ok_or(Error::IdentityNotInRoster)
    }

    /// Unwraps the epoch's root key for a member.
    pub fn unwrap_root_key(&self, identity: &Identity) -> Result<RootKey> {
        self.member_of(identity)?;
        keyring::unwrap(&self.keyring, identity, &self.cache_id, self.epoch)
    }
}

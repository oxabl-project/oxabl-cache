//! Building and changing rosters: genesis, add member, remove member, rotate.

use oxabl_cache_format::limits::ROSTER_SIGNATURES;
use oxabl_cache_format::{CacheId, PublicKey, Roster, RosterSignature};

use crate::domain::{SIG_ROSTER, prefixed};
use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::keyring;
use crate::keys::RootKey;
use crate::roster_member::{MemberEntry, check_members};
use crate::roster_verify::VerifiedRoster;

/// A roster that is not signed yet, with the root key its keyring will carry.
pub struct RosterDraft {
    cache_id: CacheId,
    version: u64,
    epoch: u32,
    members: Vec<MemberEntry>,
    root_key: RootKey,
}

impl RosterDraft {
    /// The first roster of a cache: version 1, epoch 1 and a fresh root key.
    pub fn genesis(cache_id: CacheId, members: Vec<MemberEntry>) -> Result<Self> {
        check_members(&members)?;
        Ok(Self {
            cache_id,
            version: 1,
            epoch: 1,
            members,
            root_key: RootKey::generate()?,
        })
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

    /// The root key the keyring carries; for a new epoch, the only copy outside the keyring.
    pub fn root_key(&self) -> &RootKey {
        &self.root_key
    }

    /// Signs with one to four owner identities and returns the `OXRS` bytes.
    pub fn sign(&self, owners: &[&Identity]) -> Result<Vec<u8>> {
        if !(1..=ROSTER_SIGNATURES).contains(&owners.len()) {
            return Err(Error::Roster("signature count"));
        }
        check_members(&self.members)?;
        let keyring = keyring::wrap(
            &self.root_key,
            &self.cache_id,
            self.epoch,
            self.members
                .iter()
                .flat_map(|m| m.age_recipients.iter().map(String::as_str)),
        )?;
        let mut roster = Roster {
            cache_id: self.cache_id,
            version: self.version,
            epoch: self.epoch,
            members: self.members.iter().map(MemberEntry::to_wire).collect(),
            keyring: &keyring,
            signatures: Vec::new(),
        };
        let transcript = prefixed(SIG_ROSTER, &roster.signed_bytes()?);
        let mut seen: Vec<PublicKey> = Vec::new();
        for owner in owners {
            let key = owner.signing_public();
            if seen.contains(&key) {
                return Err(Error::Roster("duplicate owner signer"));
            }
            seen.push(key);
            roster.signatures.push(RosterSignature {
                owner_key: key,
                signature: owner.sign(&transcript),
            });
        }
        Ok(roster.encode()?)
    }
}

impl VerifiedRoster {
    fn successor(
        &self,
        epoch: u32,
        members: Vec<MemberEntry>,
        root_key: RootKey,
    ) -> Result<RosterDraft> {
        check_members(&members)?;
        Ok(RosterDraft {
            cache_id: self.cache_id(),
            version: self
                .version()
                .checked_add(1)
                .ok_or(Error::Roster("version overflow"))?,
            epoch,
            members,
            root_key,
        })
    }

    /// Adds a member in the same epoch: version + 1, the same root key re-wrapped to everyone.
    pub fn draft_add_member(&self, root_key: &RootKey, member: MemberEntry) -> Result<RosterDraft> {
        let mut members = self.members().to_vec();
        members.push(member);
        self.successor(self.epoch(), members, root_key.clone())
    }

    /// Removes a member and starts the next epoch with a fresh root key they never receive.
    pub fn draft_remove_member(&self, label: &str) -> Result<RosterDraft> {
        let mut members = self.members().to_vec();
        let before = members.len();
        members.retain(|m| m.label != label);
        if members.len() == before {
            return Err(Error::Roster("no such member"));
        }
        self.draft_epoch(members)
    }

    /// Starts the next epoch for the same members.
    pub fn draft_rotate(&self) -> Result<RosterDraft> {
        self.draft_epoch(self.members().to_vec())
    }

    fn draft_epoch(&self, members: Vec<MemberEntry>) -> Result<RosterDraft> {
        let epoch = self
            .epoch()
            .checked_add(1)
            .ok_or(Error::Roster("epoch overflow"))?;
        self.successor(epoch, members, RootKey::generate()?)
    }
}

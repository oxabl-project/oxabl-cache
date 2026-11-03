use oxabl_cache_crypto::{Identity, LookupKey, RecordKey, SegmentKey, VerifiedRoster};
use oxabl_cache_format::{Namespaces, Role};

use crate::error::OpenError;
use crate::mode::Mode;

/// A verified roster with the keys of its epoch, derived for one member.
pub(crate) struct RosterView {
    pub roster: VerifiedRoster,
    pub lookup: LookupKey,
    pub segment: SegmentKey,
    pub record: RecordKey,
}

impl RosterView {
    /// Unwraps the epoch's root key for `identity` and derives the working keys from it.
    pub fn build(roster: VerifiedRoster, identity: &Identity) -> Result<Self, OpenError> {
        let root = roster.unwrap_root_key(identity).map_err(|e| match e {
            oxabl_cache_crypto::Error::IdentityNotInRoster => OpenError::IdentityNotInRoster,
            other => OpenError::Roster(other),
        })?;
        Ok(Self {
            lookup: root.lookup_key(),
            segment: root.segment_key(),
            record: root.record_key(),
            roster,
        })
    }

    pub fn epoch(&self) -> u32 {
        self.roster.epoch()
    }

    /// The role the identity has in this roster.
    pub fn role(&self, identity: &Identity) -> Option<Role> {
        self.roster.member_of(identity).ok().map(|m| m.role)
    }
}

/// The namespaces a client of `mode` and `role` reads: trusted, and its own personal one when
/// it develops.
pub(crate) fn read_namespaces(mode: Mode, role: Option<Role>) -> Namespaces {
    let personal = match mode {
        Mode::Ci => false,
        Mode::Developer => true,
        Mode::ReadOnly => role == Some(Role::Developer),
    };
    Namespaces {
        trusted: true,
        personal,
    }
}

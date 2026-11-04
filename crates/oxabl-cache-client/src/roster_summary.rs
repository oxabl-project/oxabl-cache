//! A verified roster as plain values, for reporting and for choosing what to change.

use oxabl_cache_crypto::{MemberEntry, VerifiedRoster};
use oxabl_cache_format::Role;

use crate::config::owner_key_text;

/// One member of a roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSummary {
    pub label: String,
    pub role: Role,
    /// Each signing key as `ed25519:` and unpadded base64, the spelling of `owner_keys`.
    pub signing_keys: Vec<String>,
}

impl MemberSummary {
    pub(crate) fn of(member: &MemberEntry) -> Self {
        Self {
            label: member.label.clone(),
            role: member.role,
            signing_keys: member.sign_keys.iter().map(owner_key_text).collect(),
        }
    }
}

/// The roster in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterSummary {
    pub version: u64,
    pub epoch: u32,
    pub members: Vec<MemberSummary>,
    /// The pinned owner keys whose signatures verified on this roster.
    pub signed_by: Vec<String>,
}

impl RosterSummary {
    pub(crate) fn of(roster: &VerifiedRoster) -> Self {
        Self {
            version: roster.version(),
            epoch: roster.epoch(),
            members: roster.members().iter().map(MemberSummary::of).collect(),
            signed_by: roster.signers().iter().map(owner_key_text).collect(),
        }
    }
}

/// The lower-case name of a role, as the roster and the command line spell it.
pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Ci => "ci",
        Role::Developer => "developer",
        Role::Reader => "reader",
    }
}

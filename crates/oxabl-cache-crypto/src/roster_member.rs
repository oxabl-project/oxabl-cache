//! Roster members as owned values, and the rules every roster must satisfy.

use std::collections::HashSet;

use oxabl_cache_format::limits::ROSTER_MEMBERS;
use oxabl_cache_format::{MemberDescriptor, PublicKey, Role};

use crate::error::{Error, Result};
use crate::identity_string::{age_recipient_bytes, age_recipient_string};

/// One member of a roster: a label, a role, signing keys and age recipients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberEntry {
    pub label: String,
    pub role: Role,
    pub sign_keys: Vec<PublicKey>,
    pub age_recipients: Vec<String>,
}

impl MemberEntry {
    /// A member with the one key pair that a descriptor carries.
    pub fn from_descriptor(label: &str, role: Role, descriptor: &MemberDescriptor) -> Result<Self> {
        Ok(Self {
            label: label.to_owned(),
            role,
            sign_keys: vec![descriptor.ed25519_public],
            age_recipients: vec![age_recipient_string(&descriptor.x25519_public)?],
        })
    }

    pub(crate) fn from_wire(member: &oxabl_cache_format::Member<'_>) -> Self {
        Self {
            label: member.label.to_owned(),
            role: member.role,
            sign_keys: member.sign_keys.clone(),
            age_recipients: member
                .age_recipients
                .iter()
                .map(|r| (*r).to_owned())
                .collect(),
        }
    }

    pub(crate) fn to_wire(&self) -> oxabl_cache_format::Member<'_> {
        oxabl_cache_format::Member {
            label: &self.label,
            role: self.role,
            sign_keys: self.sign_keys.clone(),
            age_recipients: self.age_recipients.iter().map(String::as_str).collect(),
        }
    }

    pub(crate) fn has_sign_key(&self, key: &PublicKey) -> bool {
        self.sign_keys.contains(key)
    }
}

/// Fails closed on a roster that no honest owner would sign: no members, too many, a member
/// without keys, a malformed recipient, or a label, signing key or recipient used twice.
pub(crate) fn check_members(members: &[MemberEntry]) -> Result<()> {
    if members.is_empty() || members.len() > ROSTER_MEMBERS {
        return Err(Error::Roster("member count"));
    }
    let mut labels = HashSet::new();
    let mut keys = HashSet::new();
    let mut recipients = HashSet::new();
    for m in members {
        if m.sign_keys.is_empty() || m.age_recipients.is_empty() {
            return Err(Error::Roster("member without keys"));
        }
        if !labels.insert(m.label.as_str()) {
            return Err(Error::Roster("duplicate label"));
        }
        for k in &m.sign_keys {
            if !keys.insert(*k) {
                return Err(Error::Roster("duplicate signing key"));
            }
        }
        for r in &m.age_recipients {
            age_recipient_bytes(r).map_err(|_| Error::Roster("malformed recipient"))?;
            if !recipients.insert(r.as_str()) {
                return Err(Error::Roster("duplicate recipient"));
            }
        }
    }
    Ok(())
}

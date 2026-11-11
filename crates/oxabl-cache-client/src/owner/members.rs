use oxabl_cache_crypto::{MemberEntry, identity_string};
use oxabl_cache_format::Role;

use super::Owner;
use super::error::OwnerError;
use super::publish::Published;

const MAX_LABEL_BYTES: usize = 128;

/// A label is 1 to 128 bytes of printable characters.
pub(super) fn check_label(label: &str) -> Result<(), OwnerError> {
    if label.is_empty() || label.len() > MAX_LABEL_BYTES || label.chars().any(char::is_control) {
        return Err(OwnerError::Invalid(
            "a label is 1 to 128 printable characters",
        ));
    }
    Ok(())
}

pub(super) fn entry(label: &str, role: Role, descriptor: &str) -> Result<MemberEntry, OwnerError> {
    check_label(label)?;
    let descriptor = identity_string::parse_descriptor(descriptor)
        .map_err(|_| OwnerError::Invalid("not a member descriptor"))?;
    Ok(MemberEntry::from_descriptor(label, role, &descriptor)?)
}

impl Owner {
    /// Checks what `add_member` checks before it signs, without publishing: the role, the label
    /// and that no member holds the label yet.
    pub fn check_new_member(&self, label: &str, role: Role) -> Result<(), OwnerError> {
        if role == Role::Owner {
            return Err(OwnerError::Invalid("owners are added with add_owner"));
        }
        check_label(label)?;
        if self.roster().members.iter().any(|m| m.label == label) {
            return Err(OwnerError::Invalid("a member with this label exists"));
        }
        Ok(())
    }

    /// Adds a `ci`, `developer` or `reader` member in the same epoch: roster `v + 1` with the
    /// root key wrapped to the new recipient set as well.
    pub async fn add_member(
        &self,
        descriptor: &str,
        label: &str,
        role: Role,
    ) -> Result<Published, OwnerError> {
        if role == Role::Owner {
            return Err(OwnerError::Invalid("owners are added with add_owner"));
        }
        let member = entry(label, role, descriptor)?;
        let inner = &self.cache.inner;
        let view = inner.view();
        let root = view.roster.unwrap_root_key(&inner.identity)?;
        let draft = view.roster.draft_add_member(&root, member)?;
        self.publish(draft, &[], inner.pins.owner_keys.clone(), false)
            .await
    }

    /// Removes a `ci`, `developer` or `reader` member and starts a new epoch whose root key
    /// they never receive. With `purge`, entries of older epochs are marked dead.
    pub async fn remove_member(&self, label: &str, purge: bool) -> Result<Published, OwnerError> {
        let inner = &self.cache.inner;
        let view = inner.view();
        let member = view
            .roster
            .members()
            .iter()
            .find(|m| m.label == label)
            .ok_or(OwnerError::NoSuchMember)?;
        if member.role == Role::Owner {
            return Err(OwnerError::Invalid("owners are removed with remove_owner"));
        }
        let draft = view.roster.draft_remove_member(label)?;
        self.publish(draft, &[], inner.pins.owner_keys.clone(), purge)
            .await
    }

    /// Starts a new epoch for the same members. With `purge`, entries of older epochs are
    /// marked dead.
    pub async fn rotate(&self, purge: bool) -> Result<Published, OwnerError> {
        let inner = &self.cache.inner;
        let draft = inner.view().roster.draft_rotate()?;
        self.publish(draft, &[], inner.pins.owner_keys.clone(), purge)
            .await
    }
}

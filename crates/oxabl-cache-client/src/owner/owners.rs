use oxabl_cache_crypto::Identity;
use oxabl_cache_format::{PublicKey, Role};

use super::Owner;
use super::error::OwnerError;
use super::members::entry;
use super::publish::Published;
use crate::config::parse_owner_key;

impl Owner {
    /// Adds an owner in the same epoch: roster `v + 1`, signed by this identity and by the new
    /// owner's own key, which proves the key is held. `descriptor` must name `cosigner`.
    pub async fn add_owner(
        &self,
        descriptor: &str,
        label: &str,
        cosigner: &Identity,
    ) -> Result<Published, OwnerError> {
        let member = entry(label, Role::Owner, descriptor)?;
        if member.sign_keys != [cosigner.signing_public()]
            || member.age_recipients != [cosigner.recipient()]
        {
            return Err(OwnerError::Invalid(
                "the descriptor does not name the cosigning identity",
            ));
        }
        let inner = &self.cache.inner;
        let view = inner.view();
        let root = view.roster.unwrap_root_key(&inner.identity)?;
        let draft = view.roster.draft_add_member(&root, member)?;
        let mut keys = inner.pins.owner_keys.clone();
        if !keys.contains(&cosigner.signing_public()) {
            keys.push(cosigner.signing_public());
        }
        self.publish(draft, &[cosigner], keys, false).await
    }

    /// Removes an owner, named by label or by an `ed25519:` signing key, and starts a new epoch
    /// they never receive. Another owner must sign it, and one must remain.
    pub async fn remove_owner(&self, owner: &str, purge: bool) -> Result<Published, OwnerError> {
        let inner = &self.cache.inner;
        let view = inner.view();
        let key = parse_owner_key(owner).ok();
        let target = view
            .roster
            .members()
            .iter()
            .find(|m| match &key {
                Some(key) => m.sign_keys.contains(key),
                None => m.label == owner,
            })
            .ok_or(OwnerError::NoSuchMember)?;
        if target.role != Role::Owner {
            return Err(OwnerError::Invalid("the member is not an owner"));
        }
        if target.sign_keys.contains(&inner.identity.signing_public()) {
            return Err(OwnerError::RemovesSigner);
        }
        let removed: Vec<PublicKey> = target.sign_keys.clone();
        let label = target.label.clone();
        let draft = view.roster.draft_remove_member(&label)?;
        let keys = inner
            .pins
            .owner_keys
            .iter()
            .filter(|k| !removed.contains(k))
            .copied()
            .collect();
        self.publish(draft, &[], keys, purge).await
    }
}

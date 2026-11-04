use bytes::Bytes;
use oxabl_cache_crypto::{Identity, RosterDraft};
use oxabl_cache_format::Role;

use super::Owner;
use super::error::OwnerError;
use crate::config::owner_key_text;

/// What became of the purge a change asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purge {
    NotRequested,
    /// Entries of older epochs the server marked dead.
    Marked(u64),
    /// The roster is published but the server did not purge: repeat the purge.
    Failed,
}

/// A roster the server accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub roster_version: u64,
    pub epoch: u32,
    /// The value `min_roster_version` takes in `.oxabl/cache.toml` to refuse every older roster.
    pub min_roster_version: u64,
    /// The `owner_keys` of `.oxabl/cache.toml` after this change, in its spelling.
    pub owner_keys: Vec<String>,
    /// The same keys as lower-case hex, the spelling the tenant API takes for a cache's owner keys.
    pub owner_keys_hex: Vec<String>,
    pub purge: Purge,
}

impl Owner {
    /// Signs `draft` with this identity and `cosigners`, publishes it over the roster held, and
    /// raises the local high-water mark. A roster without an owner is never signed.
    /// `owner_keys` are the keys the repository pins once the change is committed there.
    pub(super) async fn publish(
        &self,
        draft: RosterDraft,
        cosigners: &[&Identity],
        owner_keys: Vec<oxabl_cache_format::PublicKey>,
        purge: bool,
    ) -> Result<Published, OwnerError> {
        if !draft.members().iter().any(|m| m.role == Role::Owner) {
            return Err(OwnerError::LastOwner);
        }
        let inner = &self.cache.inner;
        let mut signers = vec![&inner.identity];
        signers.extend(cosigners);
        let bytes = draft.sign(&signers)?;
        let current = inner.view().roster.version();
        inner
            .api
            .put_roster(current, &Bytes::from(bytes))
            .await
            .map_err(OwnerError::from_api)?;
        if let Err(error) = inner.high_water.advance(draft.version()) {
            tracing::warn!(%error, "the roster high-water mark was not saved");
        }
        let purge = match purge {
            false => Purge::NotRequested,
            true => match inner.api.purge_below(draft.epoch()).await {
                Ok(marked) => Purge::Marked(marked),
                Err(_) => Purge::Failed,
            },
        };
        Ok(Published {
            roster_version: draft.version(),
            epoch: draft.epoch(),
            min_roster_version: draft.version(),
            owner_keys: owner_keys.iter().map(owner_key_text).collect(),
            owner_keys_hex: owner_keys
                .iter()
                .map(|key| key.iter().map(|b| format!("{b:02x}")).collect())
                .collect(),
            purge,
        })
    }
}

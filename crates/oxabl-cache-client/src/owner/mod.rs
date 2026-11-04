//! Changing a cache's roster as one of its owners, and creating the first roster.

mod error;
mod init;
mod members;
mod owners;
mod publish;

use oxabl_cache_format::Role;

use crate::cache::Cache;
use crate::config::{Config, owner_key_text};
use crate::roster_summary::RosterSummary;

pub use error::OwnerError;
pub use init::{InitReport, InitRequest, init};
pub use publish::{Published, Purge};

/// A cache handle whose identity is an owner, which signs the rosters it publishes.
pub struct Owner {
    cache: Cache,
}

impl Owner {
    /// Opens the cache like [`Cache::open`] and checks that the identity is an `owner` member
    /// whose signing key the repository pins.
    pub async fn open(config: Config) -> Result<Owner, OwnerError> {
        let cache = Cache::open(config).await?;
        let inner = &cache.inner;
        let owns = inner.view().role(&inner.identity) == Some(Role::Owner);
        if !owns
            || !inner
                .pins
                .owner_keys
                .contains(&inner.identity.signing_public())
        {
            return Err(OwnerError::NotAnOwner);
        }
        Ok(Owner { cache })
    }

    /// The verified roster in force.
    pub fn roster(&self) -> RosterSummary {
        RosterSummary::of(&self.cache.inner.view().roster)
    }

    /// The id of the cache.
    pub fn cache_id(&self) -> String {
        self.cache.inner.pins.cache.to_text()
    }

    /// The owner keys the repository pins, in the spelling of `owner_keys`.
    pub fn pinned_owner_keys(&self) -> Vec<String> {
        self.cache
            .inner
            .pins
            .owner_keys
            .iter()
            .map(owner_key_text)
            .collect()
    }
}

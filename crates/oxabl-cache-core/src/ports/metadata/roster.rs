//! Owner keys, rosters and the dictionary pointer.

use crate::domain::roster::{MemberRole, PublicKey, StoredDictionaryPointer, StoredRoster};
use crate::error::Result;
use crate::ids::CacheId;

/// Queries and writes of the cache's signed documents.
pub trait RosterTx {
    /// Replaces the owner keys the server checks roster signatures against.
    fn set_owner_keys(&mut self, cache: &CacheId, keys: &[PublicKey]) -> Result<()>;
    /// The registered owner keys.
    fn owner_keys(&self, cache: &CacheId) -> Result<Vec<PublicKey>>;
    /// The current roster, if any.
    fn roster(&self, cache: &CacheId) -> Result<Option<StoredRoster>>;
    /// Replaces the roster if the cache is still at `expected_version`, updates
    /// the cache's roster version and epoch, and replaces its signer keys.
    /// Returns `false`, changing nothing, if the version moved.
    fn replace_roster(
        &mut self,
        cache: &CacheId,
        expected_version: u64,
        roster: &StoredRoster,
        signers: &[(PublicKey, MemberRole)],
        now: i64,
    ) -> Result<bool>;
    /// The roles the current roster gives a signing key.
    fn signer_roles(&self, cache: &CacheId, key: &PublicKey) -> Result<Vec<MemberRole>>;
    /// The current dictionary pointer, if any.
    fn dictionary_pointer(&self, cache: &CacheId) -> Result<Option<StoredDictionaryPointer>>;
    /// Removes the pointer; the next pointer starts again at version 1.
    fn delete_dictionary_pointer(&mut self, cache: &CacheId) -> Result<()>;
    /// Replaces the pointer if it is still at `expected_version` (0 for none).
    /// Returns `false`, changing nothing, if the version moved.
    fn replace_dictionary_pointer(
        &mut self,
        cache: &CacheId,
        expected_version: u64,
        pointer: &StoredDictionaryPointer,
        now: i64,
    ) -> Result<bool>;
}

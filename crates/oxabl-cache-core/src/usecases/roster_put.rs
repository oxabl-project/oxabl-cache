//! Publishing a new roster version.

use crate::domain::principal::Caller;
use crate::domain::roster::{MemberRole, PublicKey, Roster, StoredRoster};
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::CacheId;
use crate::limits::ROSTER_BYTES;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::scoped_cache::scoped_cache;
use crate::usecases::signature_check::verifies;

const MAX_MEMBERS: usize = 1_000;
const MAX_SIGNATURES: usize = 4;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Replaces the roster. Needs `cache:admin`. `expected_version` is the
    /// `If-Match` version (`PreconditionFailed` if stale); the body must be
    /// `current + 1`, of epoch `current` or `current + 1`, and carry a valid
    /// signature by a registered owner key. Only public keys are used.
    pub async fn put_roster(
        &self,
        caller: &Caller,
        cache: CacheId,
        expected_version: u64,
        roster: Roster,
    ) -> Result<()> {
        let caller = caller.clone();
        let now = self.now();
        self.meta
            .transact(move |tx| {
                let found = scoped_cache(tx, &caller, &cache, Scope::CacheAdmin)?;
                if found.roster_version != expected_version {
                    return Err(Error::PreconditionFailed);
                }
                if roster.bytes.len() > ROSTER_BYTES {
                    return Err(Error::TooLarge);
                }
                if roster.members.len() > MAX_MEMBERS
                    || roster.signatures.is_empty()
                    || roster.signatures.len() > MAX_SIGNATURES
                {
                    return Err(Error::bad_request(
                        "roster member or signature count out of range",
                    ));
                }
                if roster.version != found.roster_version + 1 {
                    return Err(Error::bad_request("roster version must be current + 1"));
                }
                if roster.epoch != found.epoch && roster.epoch != found.epoch.saturating_add(1) {
                    return Err(Error::bad_request("roster epoch out of range"));
                }
                let owners = tx.owner_keys(&cache)?;
                let signed = roster.signatures.iter().any(|sig| {
                    owners.contains(&sig.key)
                        && verifies(&sig.key, &roster.signed_message, &sig.signature)
                });
                if !signed {
                    return Err(Error::bad_request("no valid owner signature"));
                }
                let signers: Vec<(PublicKey, MemberRole)> = roster
                    .members
                    .iter()
                    .flat_map(|m| m.sign_keys.iter().map(|key| (*key, m.role)))
                    .collect();
                let stored = StoredRoster {
                    version: roster.version,
                    epoch: roster.epoch,
                    bytes: roster.bytes,
                };
                if tx.replace_roster(&cache, expected_version, &stored, &signers, now)? {
                    Ok(())
                } else {
                    Err(Error::PreconditionFailed)
                }
            })
            .await
    }
}

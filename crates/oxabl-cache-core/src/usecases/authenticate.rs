//! Turning a bearer credential into a caller.

use crate::domain::principal::Caller;
use crate::domain::tenant::TenantStatus;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::credential_text::{hash_secret, parse};
use subtle::ConstantTimeEq;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Verifies a token or session and returns who it acts as. An unknown id, a
    /// wrong secret, an expired or revoked credential are all `Unauthenticated`;
    /// the secret hash is compared in constant time.
    pub async fn authenticate(&self, bearer: &str) -> Result<Caller> {
        let parsed = parse(bearer).ok_or(Error::Unauthenticated)?;
        let now = self.now();
        let presented = hash_secret(&parsed.secret);
        let (credential, tenant) = self
            .meta
            .read(move |tx| {
                let credential = tx.credential(parsed.kind, &parsed.id)?;
                let tenant = match &credential {
                    Some(credential) => tx.tenant(&credential.tenant)?,
                    None => None,
                };
                Ok((credential, tenant))
            })
            .await?;
        let stored = credential
            .as_ref()
            .map_or([0u8; 32], |credential| credential.secret_hash);
        let matches: bool = presented.ct_eq(&stored).into();
        let (Some(credential), Some(tenant), true) = (credential, tenant, matches) else {
            return Err(Error::Unauthenticated);
        };
        if credential.revoked_at.is_some() || now >= credential.expires_at {
            return Err(Error::Unauthenticated);
        }
        if tenant.status == TenantStatus::Suspended {
            return Err(Error::TenantSuspended);
        }
        Ok(Caller {
            tenant: credential.tenant,
            principal: credential.principal,
            grants: credential.grants,
        })
    }
}

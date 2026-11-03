//! Revoking API tokens.

use crate::domain::credential::CredentialKind;
use crate::domain::principal::Caller;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::CredentialId;
use crate::ports::TxScope;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Revokes a token of the caller's tenant at once. Needs `tenant:admin`.
    pub async fn revoke_token(&self, caller: &Caller, id: CredentialId) -> Result<()> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        let now = self.now();
        self.meta
            .transact_in(TxScope::Tenant(tenant), move |tx| {
                match tx.credential(CredentialKind::Token, &id)? {
                    Some(credential) if credential.tenant == tenant => {}
                    _ => return Err(Error::NotFound),
                }
                tx.revoke_credential(CredentialKind::Token, &id, now)?;
                Ok(())
            })
            .await
    }
}

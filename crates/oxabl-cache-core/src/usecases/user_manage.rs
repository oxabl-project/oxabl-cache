//! Human principals and their grants.

use crate::domain::principal::Caller;
use crate::domain::scope::{Grant, Scope, ScopeSet};
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::PrincipalId;
use crate::ports::TxScope;
use crate::ports::metadata::User;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Registers a user by issuer and subject. Needs `tenant:admin`.
    pub async fn create_user(
        &self,
        caller: &Caller,
        issuer: String,
        subject: String,
        label: String,
    ) -> Result<PrincipalId> {
        caller.require_tenant_admin()?;
        let user = User {
            id: PrincipalId::random(),
            tenant: caller.tenant,
            issuer,
            subject,
            label,
            created_at: self.now(),
            disabled_at: None,
        };
        let id = user.id;
        self.meta
            .transact_in(TxScope::Tenant(user.tenant), move |tx| {
                if tx.insert_user(&user)? {
                    Ok(())
                } else {
                    Err(Error::Conflict(Conflict::Exists))
                }
            })
            .await?;
        Ok(id)
    }

    /// Disables a user and revokes their credentials at once. Needs `tenant:admin`.
    pub async fn disable_user(&self, caller: &Caller, user: PrincipalId) -> Result<()> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        let now = self.now();
        self.meta
            .transact_in(TxScope::Tenant(tenant), move |tx| {
                match tx.user(&user)? {
                    Some(found) if found.tenant == tenant => {}
                    _ => return Err(Error::NotFound),
                }
                tx.disable_user(&user, now)?;
                tx.revoke_credentials_of(&user, now)?;
                Ok(())
            })
            .await
    }

    /// Sets what a principal may do on one cache, or tenant-wide. Needs `tenant:admin`.
    pub async fn set_grant(
        &self,
        caller: &Caller,
        principal: PrincipalId,
        grant: Grant,
    ) -> Result<()> {
        caller.require_tenant_admin()?;
        let tenant_admin = ScopeSet::EMPTY.with(Scope::TenantAdmin);
        let well_formed = match grant.cache {
            None => grant.scopes == tenant_admin,
            Some(_) => !grant.scopes.contains(Scope::TenantAdmin) && !grant.scopes.is_empty(),
        };
        if !well_formed {
            return Err(Error::bad_request(
                "a tenant-wide grant is tenant:admin alone; a cache grant never carries it",
            ));
        }
        let tenant = caller.tenant;
        self.meta
            .transact_in(TxScope::Tenant(tenant), move |tx| {
                match tx.user(&principal)? {
                    Some(found) if found.tenant == tenant => {}
                    _ => return Err(Error::NotFound),
                }
                if let Some(cache) = &grant.cache {
                    tx.cache(&tenant, cache)?.ok_or(Error::NotFound)?;
                }
                tx.set_grant(&tenant, &principal, &grant)
            })
            .await
    }
}

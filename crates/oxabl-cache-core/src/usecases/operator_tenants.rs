//! Tenant operations that only a service operator performs.

use crate::domain::principal::Caller;
use crate::domain::scope::{Grant, Scope, ScopeSet};
use crate::domain::tenant::{Tenant, TenantLimits};
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{PrincipalId, TenantId};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::token_create::{NewCredential, TokenSpec};

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Every tenant. An operator action.
    pub async fn list_tenants(&self) -> Result<Vec<Tenant>> {
        self.meta.transact(|tx| tx.all_tenants()).await
    }

    /// One tenant, or `NotFound`.
    pub async fn get_tenant(&self, id: TenantId) -> Result<Tenant> {
        self.meta
            .transact(move |tx| tx.tenant(&id)?.ok_or(Error::NotFound))
            .await
    }

    /// Replaces a tenant's quotas and rates. An operator action.
    pub async fn set_tenant_limits(&self, id: TenantId, limits: TenantLimits) -> Result<()> {
        if limits.rate == 0 || limits.burst == 0 {
            return Err(Error::bad_request("rate and burst must be positive"));
        }
        self.meta
            .transact(move |tx| {
                if tx.set_tenant_limits(&id, &limits)? {
                    Ok(())
                } else {
                    Err(Error::NotFound)
                }
            })
            .await
    }

    /// Creates the first administrator token of a tenant. An operator action:
    /// the token carries `tenant:admin` and nothing else.
    pub async fn create_tenant_admin_token(
        &self,
        tenant: TenantId,
        name: String,
        expires_in_secs: Option<i64>,
    ) -> Result<NewCredential> {
        self.get_tenant(tenant).await?;
        let acting = Caller {
            tenant,
            principal: PrincipalId::random(),
            grants: vec![Grant {
                cache: None,
                scopes: ScopeSet::EMPTY.with(Scope::TenantAdmin),
            }],
        };
        self.create_token(
            &acting,
            TokenSpec {
                name,
                principal: None,
                scopes: ScopeSet::EMPTY.with(Scope::TenantAdmin),
                caches: Vec::new(),
                expires_in_secs,
            },
        )
        .await
    }
}

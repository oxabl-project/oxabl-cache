//! Creating and suspending tenants.

use crate::domain::tenant::{Tenant, TenantStatus};
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::TenantId;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

/// The limits of a new tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantSpec {
    /// Operator-visible name.
    pub name: String,
    /// Hard limit on live plus pending bytes across the tenant's caches.
    pub hard_bytes: Option<u64>,
    /// Monthly egress limit.
    pub egress_bytes: Option<u64>,
    /// Requests per second.
    pub rate: u32,
    /// Burst size.
    pub burst: u32,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Creates a tenant (an operator action). A name is unique: a second tenant of
    /// that name is `Conflict(Exists)`.
    pub async fn create_tenant(&self, spec: TenantSpec) -> Result<TenantId> {
        if spec.name.is_empty() {
            return Err(Error::bad_request("a tenant needs a name"));
        }
        let tenant = Tenant {
            id: TenantId::random(),
            name: spec.name,
            status: TenantStatus::Active,
            hard_bytes: spec.hard_bytes,
            egress_bytes: spec.egress_bytes,
            rate: spec.rate,
            burst: spec.burst,
            created_at: self.now(),
        };
        let id = tenant.id;
        self.meta
            .transact(move |tx| {
                if tx
                    .all_tenants()?
                    .iter()
                    .any(|existing| existing.name == tenant.name)
                {
                    return Err(Error::Conflict(Conflict::Exists));
                }
                if tx.insert_tenant(&tenant)? {
                    Ok(())
                } else {
                    Err(Error::Conflict(Conflict::Exists))
                }
            })
            .await?;
        Ok(id)
    }

    /// Suspends or resumes a tenant (an operator action). Every credential of a
    /// suspended tenant is refused at once.
    pub async fn set_tenant_status(&self, tenant: TenantId, status: TenantStatus) -> Result<()> {
        self.meta
            .transact(move |tx| {
                if tx.set_tenant_status(&tenant, status)? {
                    Ok(())
                } else {
                    Err(Error::NotFound)
                }
            })
            .await
    }
}

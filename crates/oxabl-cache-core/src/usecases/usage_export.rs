//! Reading the ledger for a calendar month.

use crate::domain::principal::Caller;
use crate::domain::usage::UsageTotal;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::time::month_days;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// The ledger summed per tenant, cache and metric for a UTC month (`YYYY-MM`).
    /// An operator action: it spans every tenant.
    pub async fn usage_export(&self, month: &str) -> Result<Vec<UsageTotal>> {
        let (from, to) = month_days(month).ok_or_else(|| Error::bad_request("month is YYYY-MM"))?;
        self.meta.read(move |tx| tx.usage_totals(from, to)).await
    }

    /// The caller's own tenant's usage for a UTC month. Needs `tenant:admin`.
    pub async fn tenant_usage(&self, caller: &Caller, month: &str) -> Result<Vec<UsageTotal>> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        let totals = self.usage_export(month).await?;
        Ok(totals.into_iter().filter(|t| t.tenant == tenant).collect())
    }
}

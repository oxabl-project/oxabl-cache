//! The transactional metadata port.

mod accounts;
mod admin;
mod entries;
mod ledger;
mod oidc;
mod packs;
mod roster;
mod segments;

pub use accounts::{AccountsTx, User};
pub use admin::AdminTx;
pub use entries::{EntriesTx, EvictionScope};
pub use ledger::LedgerTx;
pub use oidc::OidcTx;
pub use packs::{PacksTx, RecordedCommit};
pub use roster::RosterTx;
pub use segments::SegmentsTx;

use crate::error::Result;
use crate::ids::{CacheId, TenantId};
use std::future::Future;

/// Every operation available inside one metadata transaction.
pub trait MetaTx:
    AccountsTx + AdminTx + OidcTx + RosterTx + PacksTx + SegmentsTx + EntriesTx + LedgerTx
{
}

impl<T> MetaTx for T where
    T: AccountsTx + AdminTx + OidcTx + RosterTx + PacksTx + SegmentsTx + EntriesTx + LedgerTx
{
}

/// The rows a transaction writes, so a store can lock only what it must. A
/// transaction never writes rows outside its scope; its reads outside the scope
/// are not isolated from concurrent writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxScope {
    /// Any rows: excludes every other writer.
    Global,
    /// The rows of one cache.
    Cache(CacheId),
    /// The rows of one cache, and the byte totals of the tenant's other caches
    /// (tenant quota admission).
    TenantCache(TenantId, CacheId),
    /// The rows of one tenant: its users, grants, credentials and cache names.
    Tenant(TenantId),
    /// The usage ledger and its day roll-up.
    Ledger,
    /// One row whose update does not depend on any other row.
    Independent,
}

/// A metadata database that runs closures as serialisable transactions.
pub trait MetadataStore: Send + Sync + 'static {
    /// Runs `work` in one transaction: it commits when `work` returns `Ok` and
    /// rolls back when it returns `Err`. Writers whose scopes overlap never
    /// interleave; writers of disjoint scopes may run concurrently.
    fn transact_in<T, F>(&self, scope: TxScope, work: F) -> impl Future<Output = Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static;

    /// [`MetadataStore::transact_in`] over [`TxScope::Global`].
    fn transact<T, F>(&self, work: F) -> impl Future<Output = Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        self.transact_in(TxScope::Global, work)
    }

    /// Runs `work` against one consistent snapshot. `work` must not write; a store
    /// that cannot serve snapshots cheaply runs it as a global transaction.
    fn read<T, F>(&self, work: F) -> impl Future<Output = Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        self.transact(work)
    }
}

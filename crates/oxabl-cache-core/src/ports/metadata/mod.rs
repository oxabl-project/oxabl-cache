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

/// A metadata database that runs closures as serialisable transactions.
pub trait MetadataStore: Send + Sync + 'static {
    /// Runs `work` in one transaction: it commits when `work` returns `Ok` and
    /// rolls back when it returns `Err`. Writers never interleave.
    fn transact<T, F>(&self, work: F) -> impl Future<Output = Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static;
}

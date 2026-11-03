//! The transaction handle and the port implementations on it.

mod accounts;
mod admin;
mod entries;
mod fence;
mod ledger;
mod oidc;
mod operator_tokens;
mod packs;
mod roster;
mod segments;
mod teardown;

use rusqlite::Connection;

/// A transaction on the writer connection; the surrounding `BEGIN`/`COMMIT` is
/// owned by the store.
pub(crate) struct SqliteTx<'c> {
    conn: &'c Connection,
}

impl<'c> SqliteTx<'c> {
    pub(crate) fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }
}

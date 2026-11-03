//! The transaction handle: synchronous calls driving the asynchronous driver.

mod accounts;
mod admin;
mod entries;
mod fence;
mod ledger;
mod oidc;
mod packs;
mod roster;
mod segments;
mod teardown;

use crate::error::DbResult;
use crate::pool::Conn;
use oxabl_cache_core::{Error, Result};
use std::cell::Cell;
use tokio::runtime::Handle;
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

/// Bound parameters of one statement.
pub(crate) type Args<'a> = &'a [&'a (dyn ToSql + Sync)];

/// Builds [`Args`] from expressions.
macro_rules! args {
    ($($value:expr),* $(,)?) => {
        &[$(&$value as &(dyn tokio_postgres::types::ToSql + Sync)),*]
    };
}
pub(crate) use args;

/// A transaction on one pooled connection; `BEGIN` and `COMMIT` belong to the
/// store. A failed statement poisons the transaction, which the store then refuses
/// to commit.
pub(crate) struct PgTx<'c> {
    conn: &'c Conn,
    handle: &'c Handle,
    failed: Cell<bool>,
}

impl<'c> PgTx<'c> {
    pub(crate) fn new(conn: &'c Conn, handle: &'c Handle) -> Self {
        Self {
            conn,
            handle,
            failed: Cell::new(false),
        }
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed.get()
    }

    fn track<T>(&self, result: std::result::Result<T, tokio_postgres::Error>) -> Result<T> {
        let result = result.db();
        if result.is_err() {
            self.failed.set(true);
        }
        result
    }

    fn statement(&self, sql: &str) -> Result<tokio_postgres::Statement> {
        if let Some(statement) = self.conn.cached(sql) {
            return Ok(statement);
        }
        let statement = self.track(self.handle.block_on(self.conn.client.prepare(sql)))?;
        self.conn.remember(sql, &statement);
        Ok(statement)
    }

    pub(crate) fn query(&self, sql: &str, args: Args<'_>) -> Result<Vec<Row>> {
        let statement = self.statement(sql)?;
        self.track(
            self.handle
                .block_on(self.conn.client.query(&statement, args)),
        )
    }

    pub(crate) fn query_opt(&self, sql: &str, args: Args<'_>) -> Result<Option<Row>> {
        let mut rows = self.query(sql, args)?;
        match rows.len() {
            0 => Ok(None),
            1 => Ok(rows.pop()),
            _ => Err(Error::internal(
                "a statement meant for one row returned more",
            )),
        }
    }

    pub(crate) fn query_one(&self, sql: &str, args: Args<'_>) -> Result<Row> {
        self.query_opt(sql, args)?
            .ok_or_else(|| Error::internal("a statement meant for one row returned none"))
    }

    pub(crate) fn execute(&self, sql: &str, args: Args<'_>) -> Result<u64> {
        let statement = self.statement(sql)?;
        self.track(
            self.handle
                .block_on(self.conn.client.execute(&statement, args)),
        )
    }
}

/// Typed column readers.
pub(crate) trait RowExt {
    fn blob<const N: usize>(&self, index: usize) -> Result<[u8; N]>;
    fn opt_blob<const N: usize>(&self, index: usize) -> Result<Option<[u8; N]>>;
    fn bytes(&self, index: usize) -> Result<Vec<u8>>;
    fn opt_bytes(&self, index: usize) -> Result<Option<Vec<u8>>>;
    fn int(&self, index: usize) -> Result<i64>;
    fn opt_int(&self, index: usize) -> Result<Option<i64>>;
    fn unsigned(&self, index: usize) -> Result<u64>;
    fn opt_unsigned(&self, index: usize) -> Result<Option<u64>>;
    fn small(&self, index: usize) -> Result<u32>;
    fn text(&self, index: usize) -> Result<String>;
    fn opt_text(&self, index: usize) -> Result<Option<String>>;
    fn flag(&self, index: usize) -> Result<bool>;
}

fn malformed(what: &str) -> Error {
    Error::internal(format!("stored {what} is malformed"))
}

impl RowExt for Row {
    fn blob<const N: usize>(&self, index: usize) -> Result<[u8; N]> {
        self.bytes(index)?
            .try_into()
            .map_err(|_| malformed("identifier"))
    }

    fn opt_blob<const N: usize>(&self, index: usize) -> Result<Option<[u8; N]>> {
        self.opt_bytes(index)?
            .map(|bytes| bytes.try_into().map_err(|_| malformed("identifier")))
            .transpose()
    }

    fn bytes(&self, index: usize) -> Result<Vec<u8>> {
        self.try_get(index).db()
    }

    fn opt_bytes(&self, index: usize) -> Result<Option<Vec<u8>>> {
        self.try_get(index).db()
    }

    fn int(&self, index: usize) -> Result<i64> {
        self.try_get(index).db()
    }

    fn opt_int(&self, index: usize) -> Result<Option<i64>> {
        self.try_get(index).db()
    }

    fn unsigned(&self, index: usize) -> Result<u64> {
        u64::try_from(self.int(index)?).map_err(|_| malformed("number"))
    }

    fn opt_unsigned(&self, index: usize) -> Result<Option<u64>> {
        self.opt_int(index)?
            .map(|value| u64::try_from(value).map_err(|_| malformed("number")))
            .transpose()
    }

    fn small(&self, index: usize) -> Result<u32> {
        u32::try_from(self.int(index)?).map_err(|_| malformed("number"))
    }

    fn text(&self, index: usize) -> Result<String> {
        self.try_get(index).db()
    }

    fn opt_text(&self, index: usize) -> Result<Option<String>> {
        self.try_get(index).db()
    }

    fn flag(&self, index: usize) -> Result<bool> {
        self.try_get(index).db()
    }
}

/// An unsigned value as a Postgres integer, saturating at the largest.
pub(crate) fn int(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Raw bytes of ids, as the elements of a `bytea[]` parameter.
pub(crate) fn byte_array<'a>(items: impl Iterator<Item = &'a [u8]>) -> Vec<Vec<u8>> {
    items.map(<[u8]>::to_vec).collect()
}

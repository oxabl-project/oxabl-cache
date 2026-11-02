//! Mapping SQLite results onto the core error and typed row access.

use oxabl_cache_core::{Error, Result};
use rusqlite::types::Type;
use rusqlite::{ErrorCode, Row};

/// Converts a rusqlite result: a constraint violation is an internal error (an
/// invariant the schema guards), anything else makes the store unavailable.
pub(crate) trait DbResult<T> {
    fn db(self) -> Result<T>;
}

impl<T> DbResult<T> for rusqlite::Result<T> {
    fn db(self) -> Result<T> {
        self.map_err(|error| match error.sqlite_error_code() {
            Some(ErrorCode::ConstraintViolation) => Error::internal(format!("constraint: {error}")),
            _ => Error::unavailable(format!("metadata store: {error}")),
        })
    }
}

fn corrupt(index: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Blob,
        format!("stored {what} is malformed").into(),
    )
}

/// Typed column readers.
pub(crate) trait RowExt {
    fn blob<const N: usize>(&self, index: usize) -> rusqlite::Result<[u8; N]>;
    fn opt_blob<const N: usize>(&self, index: usize) -> rusqlite::Result<Option<[u8; N]>>;
    fn unsigned(&self, index: usize) -> rusqlite::Result<u64>;
    fn opt_unsigned(&self, index: usize) -> rusqlite::Result<Option<u64>>;
    fn small(&self, index: usize) -> rusqlite::Result<u32>;
}

impl RowExt for Row<'_> {
    fn blob<const N: usize>(&self, index: usize) -> rusqlite::Result<[u8; N]> {
        let bytes: Vec<u8> = self.get(index)?;
        bytes.try_into().map_err(|_| corrupt(index, "identifier"))
    }

    fn opt_blob<const N: usize>(&self, index: usize) -> rusqlite::Result<Option<[u8; N]>> {
        let bytes: Option<Vec<u8>> = self.get(index)?;
        bytes
            .map(|bytes| bytes.try_into().map_err(|_| corrupt(index, "identifier")))
            .transpose()
    }

    fn unsigned(&self, index: usize) -> rusqlite::Result<u64> {
        let value: i64 = self.get(index)?;
        u64::try_from(value).map_err(|_| corrupt(index, "number"))
    }

    fn opt_unsigned(&self, index: usize) -> rusqlite::Result<Option<u64>> {
        let value: Option<i64> = self.get(index)?;
        value
            .map(|value| u64::try_from(value).map_err(|_| corrupt(index, "number")))
            .transpose()
    }

    fn small(&self, index: usize) -> rusqlite::Result<u32> {
        let value: i64 = self.get(index)?;
        u32::try_from(value).map_err(|_| corrupt(index, "number"))
    }
}

/// An unsigned value as an SQLite integer, saturating at the largest.
pub(crate) fn int(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// A comma-separated list of `count` numbered placeholders starting at `first`.
pub(crate) fn placeholders(first: usize, count: usize) -> String {
    (first..first + count)
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(",")
}

//! Mapping Postgres errors onto the core error.

use oxabl_cache_core::{Error, Result};

/// Converts a driver result: a violated constraint is an internal error (an
/// invariant the schema guards), anything else makes the store unavailable.
pub(crate) trait DbResult<T> {
    fn db(self) -> Result<T>;
}

impl<T> DbResult<T> for std::result::Result<T, tokio_postgres::Error> {
    fn db(self) -> Result<T> {
        self.map_err(|error| match error.as_db_error() {
            Some(db) if db.code().code().starts_with("23") => {
                Error::internal(format!("constraint: {}", db.message()))
            }
            Some(db) => Error::unavailable(format!("metadata store: {}", db.message())),
            None => Error::unavailable(format!("metadata store: {error}")),
        })
    }
}

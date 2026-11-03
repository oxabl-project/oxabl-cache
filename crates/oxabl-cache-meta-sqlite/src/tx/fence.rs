//! The garbage-collection leader epoch.

use super::SqliteTx;
use crate::db::{DbResult, RowExt};
use oxabl_cache_core::ports::metadata::FenceTx;
use oxabl_cache_core::{Error, Result};

impl FenceTx for SqliteTx<'_> {
    fn advance_gc_epoch(&mut self) -> Result<u64> {
        self.conn
            .query_row(
                "UPDATE gc_leader SET epoch = epoch + 1 WHERE id = 1 RETURNING epoch",
                [],
                |row| row.unsigned(0),
            )
            .db()
    }

    fn require_gc_epoch(&mut self, epoch: u64) -> Result<()> {
        let current: u64 = self
            .conn
            .query_row("SELECT epoch FROM gc_leader WHERE id = 1", [], |row| {
                row.unsigned(0)
            })
            .db()?;
        if current == epoch {
            Ok(())
        } else {
            Err(Error::Fenced)
        }
    }
}

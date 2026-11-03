//! The garbage-collection leader epoch.

use super::{PgTx, RowExt, args};
use oxabl_cache_core::ports::metadata::FenceTx;
use oxabl_cache_core::{Error, Result};

impl FenceTx for PgTx<'_> {
    fn advance_gc_epoch(&mut self) -> Result<u64> {
        self.query_one(
            "UPDATE gc_leader SET epoch = epoch + 1 WHERE id = 1 RETURNING epoch",
            args![],
        )?
        .unsigned(0)
    }

    fn require_gc_epoch(&mut self, epoch: u64) -> Result<()> {
        let current = self
            .query_one(
                "SELECT epoch FROM gc_leader WHERE id = 1 FOR SHARE",
                args![],
            )?
            .unsigned(0)?;
        if current == epoch {
            Ok(())
        } else {
            Err(Error::Fenced)
        }
    }
}

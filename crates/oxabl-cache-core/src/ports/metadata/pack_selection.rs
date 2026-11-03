//! Pack selection.

use crate::domain::pack::Pack;
use crate::error::Result;

/// Pack selection for the sweep and compaction.
pub trait PackSelectionTx {
    /// Packs ready for the sweep: dead longer than the grace, pending with an
    /// expired lease, or already deleting.
    fn packs_due_for_sweep(&self, now: i64, grace: i64, limit: usize) -> Result<Vec<Pack>>;
    /// Live packs created before `created_before` whose live share is below
    /// `max_live_permille` thousandths, at least `min_size` bytes large, with live bytes left.
    fn compaction_candidates(
        &self,
        created_before: i64,
        max_live_permille: u64,
        min_size: u64,
        limit: usize,
    ) -> Result<Vec<Pack>>;
    /// Live packs whose live bytes are zero but that carry no `dead_since`.
    fn empty_unmarked_packs(&self, limit: usize) -> Result<Vec<Pack>>;
}

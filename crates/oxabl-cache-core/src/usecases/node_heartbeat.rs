//! The node heartbeat that sizes per-node rate limits.

use crate::engine::Engine;
use crate::error::Result;
use crate::ports::TxScope;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

const LIVE_WINDOW_SECS: i64 = 30;
const PRUNE_AFTER_SECS: i64 = 24 * 60 * 60;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Records that this node is alive and prunes long-silent nodes.
    pub async fn heartbeat(&self) -> Result<()> {
        let (node, now) = (self.usage.node(), self.now());
        self.meta
            .transact_in(TxScope::Independent, move |tx| {
                tx.heartbeat(&node, now)?;
                tx.prune_nodes(now - PRUNE_AFTER_SECS)?;
                Ok(())
            })
            .await
    }

    /// Nodes that beat within the last 30 seconds.
    pub async fn live_node_count(&self) -> Result<u32> {
        let since = self.now() - LIVE_WINDOW_SECS;
        self.meta.read(move |tx| tx.live_nodes(since)).await
    }
}

//! The usage ledger and the node heartbeat.

use super::{PgTx, RowExt, args, int};
use oxabl_cache_core::domain::usage::{Metric, ROLLED_UP_NODE, UsageRow, UsageTotal};
use oxabl_cache_core::ids::{CacheId, NodeId, TenantId};
use oxabl_cache_core::ports::metadata::LedgerTx;
use oxabl_cache_core::{Error, Result};

fn metric_of(name: &str) -> Result<Metric> {
    Metric::parse(name).ok_or_else(|| Error::internal("stored metric is unknown"))
}

impl LedgerTx for PgTx<'_> {
    fn ledger_high_water(&self, node: &NodeId) -> Result<Option<u64>> {
        self.query_opt(
            "SELECT max_flush_seq FROM ledger_nodes WHERE node_id = $1",
            args![node.as_bytes().as_slice()],
        )?
        .map(|row| row.unsigned(0))
        .transpose()
    }

    fn set_ledger_high_water(&mut self, node: &NodeId, seq: u64, now: i64) -> Result<()> {
        self.execute(
            "INSERT INTO ledger_nodes (node_id, max_flush_seq, flushed_at) VALUES ($1, $2, $3)
             ON CONFLICT (node_id) DO UPDATE SET
               max_flush_seq = excluded.max_flush_seq, flushed_at = excluded.flushed_at",
            args![node.as_bytes().as_slice(), int(seq), now],
        )?;
        Ok(())
    }

    fn day_closed(&self, day: i64) -> Result<bool> {
        self.query_one(
            "SELECT EXISTS (SELECT 1 FROM ledger_days WHERE day = $1)",
            args![day],
        )?
        .flag(0)
    }

    fn insert_ledger_rows(&mut self, node: &NodeId, seq: u64, rows: &[UsageRow]) -> Result<()> {
        for row in rows {
            self.execute(
                "INSERT INTO usage_ledger (node_id, flush_seq, tenant_id, cache_id, day, metric, value)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                args![
                    node.as_bytes().as_slice(),
                    int(seq),
                    row.tenant.as_bytes().as_slice(),
                    row.cache.as_bytes().as_slice(),
                    row.day,
                    row.metric.name(),
                    int(row.value)
                ],
            )?;
        }
        Ok(())
    }

    fn open_days(&self) -> Result<Vec<i64>> {
        self.query(
            "SELECT DISTINCT day FROM usage_ledger
             WHERE day NOT IN (SELECT day FROM ledger_days) ORDER BY day",
            args![],
        )?
        .iter()
        .map(|row| row.int(0))
        .collect()
    }

    fn roll_up_day(&mut self, day: i64, now: i64) -> Result<()> {
        let sums = self
            .query(
                "SELECT tenant_id, cache_id, metric, SUM(value)::bigint FROM usage_ledger
                 WHERE day = $1 GROUP BY tenant_id, cache_id, metric",
                args![day],
            )?
            .iter()
            .map(|row| {
                Ok(UsageRow {
                    tenant: TenantId::from_bytes(row.blob(0)?),
                    cache: CacheId::from_bytes(row.blob(1)?),
                    day,
                    metric: metric_of(&row.text(2)?)?,
                    value: row.unsigned(3)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.execute("DELETE FROM usage_ledger WHERE day = $1", args![day])?;
        self.insert_ledger_rows(&ROLLED_UP_NODE, 0, &sums)?;
        let closed = self.execute(
            "INSERT INTO ledger_days (day, closed_at) VALUES ($1, $2) ON CONFLICT (day) DO NOTHING",
            args![day, now],
        )?;
        if closed == 1 {
            Ok(())
        } else {
            Err(Error::internal("ledger day closed twice"))
        }
    }

    fn has_ledger_row(&self, cache: &CacheId, day: i64, metric: Metric) -> Result<bool> {
        self.query_one(
            "SELECT EXISTS (SELECT 1 FROM usage_ledger
                            WHERE cache_id = $1 AND day = $2 AND metric = $3)",
            args![cache.as_bytes().as_slice(), day, metric.name()],
        )?
        .flag(0)
    }

    fn usage_totals(&self, from: i64, to: i64) -> Result<Vec<UsageTotal>> {
        self.query(
            "SELECT tenant_id, cache_id, metric, SUM(value)::bigint FROM usage_ledger
             WHERE day >= $1 AND day < $2
             GROUP BY tenant_id, cache_id, metric ORDER BY tenant_id, cache_id, metric",
            args![from, to],
        )?
        .iter()
        .map(|row| {
            Ok(UsageTotal {
                tenant: TenantId::from_bytes(row.blob(0)?),
                cache: CacheId::from_bytes(row.blob(1)?),
                metric: metric_of(&row.text(2)?)?,
                value: row.unsigned(3)?,
            })
        })
        .collect()
    }

    fn tenant_metric(&self, tenant: &TenantId, metric: Metric, from: i64, to: i64) -> Result<u64> {
        self.query_one(
            "SELECT COALESCE(SUM(value), 0)::bigint FROM usage_ledger
             WHERE tenant_id = $1 AND metric = $2 AND day >= $3 AND day < $4",
            args![tenant.as_bytes().as_slice(), metric.name(), from, to],
        )?
        .unsigned(0)
    }

    fn prune_ledger_nodes(&mut self, before: i64) -> Result<u64> {
        self.execute(
            "DELETE FROM ledger_nodes WHERE flushed_at < $1",
            args![before],
        )
    }

    fn heartbeat(&mut self, node: &NodeId, now: i64) -> Result<()> {
        self.execute(
            "INSERT INTO nodes (node_id, heartbeat_at) VALUES ($1, $2)
             ON CONFLICT (node_id) DO UPDATE SET heartbeat_at = excluded.heartbeat_at",
            args![node.as_bytes().as_slice(), now],
        )?;
        Ok(())
    }

    fn live_nodes(&self, since: i64) -> Result<u32> {
        let count = self
            .query_one(
                "SELECT COUNT(*) FROM nodes WHERE heartbeat_at >= $1",
                args![since],
            )?
            .int(0)?;
        u32::try_from(count).map_err(|_| Error::internal("node count out of range"))
    }

    fn prune_nodes(&mut self, before: i64) -> Result<u64> {
        self.execute("DELETE FROM nodes WHERE heartbeat_at < $1", args![before])
    }
}

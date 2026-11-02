//! The usage ledger and the node heartbeat.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, int};
use oxabl_cache_core::domain::usage::{Metric, ROLLED_UP_NODE, UsageRow, UsageTotal};
use oxabl_cache_core::ids::{CacheId, NodeId, TenantId};
use oxabl_cache_core::ports::metadata::LedgerTx;
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, params};

fn metric_of(name: &str) -> rusqlite::Result<Metric> {
    Metric::parse(name).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            "stored metric is unknown".into(),
        )
    })
}

impl LedgerTx for SqliteTx<'_> {
    fn ledger_high_water(&self, node: &NodeId) -> Result<Option<u64>> {
        self.conn
            .prepare_cached("SELECT max_flush_seq FROM ledger_nodes WHERE node_id = ?1")
            .db()?
            .query_row([node.as_bytes().as_slice()], |row| row.unsigned(0))
            .optional()
            .db()
    }

    fn set_ledger_high_water(&mut self, node: &NodeId, seq: u64, now: i64) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO ledger_nodes (node_id, max_flush_seq, flushed_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (node_id) DO UPDATE SET
                   max_flush_seq = excluded.max_flush_seq, flushed_at = excluded.flushed_at",
            )
            .db()?
            .execute(params![node.as_bytes().as_slice(), int(seq), now])
            .db()?;
        Ok(())
    }

    fn day_closed(&self, day: i64) -> Result<bool> {
        self.conn
            .prepare_cached("SELECT EXISTS (SELECT 1 FROM ledger_days WHERE day = ?1)")
            .db()?
            .query_row([day], |row| row.get::<_, bool>(0))
            .db()
    }

    fn insert_ledger_rows(&mut self, node: &NodeId, seq: u64, rows: &[UsageRow]) -> Result<()> {
        let mut statement = self
            .conn
            .prepare_cached(
                "INSERT INTO usage_ledger (node_id, flush_seq, tenant_id, cache_id, day, metric, value)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .db()?;
        for row in rows {
            statement
                .execute(params![
                    node.as_bytes().as_slice(),
                    int(seq),
                    row.tenant.as_bytes().as_slice(),
                    row.cache.as_bytes().as_slice(),
                    row.day,
                    row.metric.name(),
                    int(row.value)
                ])
                .db()?;
        }
        Ok(())
    }

    fn open_days(&self) -> Result<Vec<i64>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT DISTINCT day FROM usage_ledger
                 WHERE day NOT IN (SELECT day FROM ledger_days) ORDER BY day",
            )
            .db()?;
        let rows = statement.query_map([], |row| row.get(0)).db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn roll_up_day(&mut self, day: i64, now: i64) -> Result<()> {
        let sums: Vec<(TenantId, CacheId, String, u64)> = {
            let mut statement = self
                .conn
                .prepare_cached(
                    "SELECT tenant_id, cache_id, metric, SUM(value) FROM usage_ledger
                     WHERE day = ?1 GROUP BY tenant_id, cache_id, metric",
                )
                .db()?;
            let rows = statement
                .query_map([day], |row| {
                    Ok((
                        TenantId::from_bytes(row.blob(0)?),
                        CacheId::from_bytes(row.blob(1)?),
                        row.get::<_, String>(2)?,
                        row.unsigned(3)?,
                    ))
                })
                .db()?;
            rows.collect::<rusqlite::Result<_>>().db()?
        };
        self.conn
            .execute("DELETE FROM usage_ledger WHERE day = ?1", [day])
            .db()?;
        let rolled: Vec<UsageRow> = sums
            .into_iter()
            .map(|(tenant, cache, metric, value)| {
                Ok(UsageRow {
                    tenant,
                    cache,
                    day,
                    metric: metric_of(&metric).db()?,
                    value,
                })
            })
            .collect::<Result<_>>()?;
        self.insert_ledger_rows(&ROLLED_UP_NODE, 0, &rolled)?;
        let closed = self
            .conn
            .execute(
                "INSERT INTO ledger_days (day, closed_at) VALUES (?1, ?2) ON CONFLICT (day) DO NOTHING",
                params![day, now],
            )
            .db()?;
        if closed == 1 {
            Ok(())
        } else {
            Err(Error::internal("ledger day closed twice"))
        }
    }

    fn has_ledger_row(&self, cache: &CacheId, day: i64, metric: Metric) -> Result<bool> {
        self.conn
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM usage_ledger
                                WHERE cache_id = ?1 AND day = ?2 AND metric = ?3)",
            )
            .db()?
            .query_row(
                params![cache.as_bytes().as_slice(), day, metric.name()],
                |row| row.get::<_, bool>(0),
            )
            .db()
    }

    fn usage_totals(&self, from: i64, to: i64) -> Result<Vec<UsageTotal>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT tenant_id, cache_id, metric, SUM(value) FROM usage_ledger
                 WHERE day >= ?1 AND day < ?2
                 GROUP BY tenant_id, cache_id, metric ORDER BY tenant_id, cache_id, metric",
            )
            .db()?;
        let rows = statement
            .query_map([from, to], |row| {
                Ok(UsageTotal {
                    tenant: TenantId::from_bytes(row.blob(0)?),
                    cache: CacheId::from_bytes(row.blob(1)?),
                    metric: metric_of(&row.get::<_, String>(2)?)?,
                    value: row.unsigned(3)?,
                })
            })
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn tenant_metric(&self, tenant: &TenantId, metric: Metric, from: i64, to: i64) -> Result<u64> {
        self.conn
            .prepare_cached(
                "SELECT COALESCE(SUM(value), 0) FROM usage_ledger
                 WHERE tenant_id = ?1 AND metric = ?2 AND day >= ?3 AND day < ?4",
            )
            .db()?
            .query_row(
                params![tenant.as_bytes().as_slice(), metric.name(), from, to],
                |row| row.unsigned(0),
            )
            .db()
    }

    fn prune_ledger_nodes(&mut self, before: i64) -> Result<u64> {
        let deleted = self
            .conn
            .execute("DELETE FROM ledger_nodes WHERE flushed_at < ?1", [before])
            .db()?;
        Ok(u64::try_from(deleted).unwrap_or(u64::MAX))
    }

    fn heartbeat(&mut self, node: &NodeId, now: i64) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO nodes (node_id, heartbeat_at) VALUES (?1, ?2)
                 ON CONFLICT (node_id) DO UPDATE SET heartbeat_at = excluded.heartbeat_at",
            )
            .db()?
            .execute(params![node.as_bytes().as_slice(), now])
            .db()?;
        Ok(())
    }

    fn live_nodes(&self, since: i64) -> Result<u32> {
        self.conn
            .prepare_cached("SELECT COUNT(*) FROM nodes WHERE heartbeat_at >= ?1")
            .db()?
            .query_row([since], |row| row.small(0))
            .db()
    }

    fn prune_nodes(&mut self, before: i64) -> Result<u64> {
        let deleted = self
            .conn
            .execute("DELETE FROM nodes WHERE heartbeat_at < ?1", [before])
            .db()?;
        Ok(u64::try_from(deleted).unwrap_or(u64::MAX))
    }
}

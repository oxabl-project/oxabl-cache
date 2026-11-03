//! Background work of a node: heartbeat, flushes of usage and access days, and the
//! GC jobs while the node leads.

use crate::config::Gc;
use crate::meta::Election;
use crate::wiring::Prod;
use oxabl_cache_core::ports::{Clock, MetadataStore};
use oxabl_cache_http::App;
use oxabl_cache_http::log::{Level, emit};
use serde_json::Value;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval, interval_at};

const HEARTBEAT: Duration = Duration::from_secs(10);
const ACCESS_FLUSH: Duration = Duration::from_secs(10);
const USAGE_FLUSH: Duration = Duration::from_secs(60);
const ELECTION: Duration = Duration::from_secs(1);

async fn job<T>(name: &str, work: impl Future<Output = oxabl_cache_core::Result<T>>) -> Option<T> {
    match work.await {
        Ok(done) => Some(done),
        Err(error) => {
            emit(
                Level::Warn,
                "background_job_failed",
                vec![
                    ("job", Value::from(name)),
                    ("error", Value::from(error.to_string())),
                ],
            );
            None
        }
    }
}

/// Flushes access days and usage counters; used on a timer and once more at shutdown.
pub(crate) async fn flush_counters(app: &App<Prod>) {
    job("access_flush", app.engine.flush_access_days()).await;
    job("usage_flush", app.flush_usage()).await;
}

async fn flush_loop(app: Arc<App<Prod>>, mut stop: watch::Receiver<bool>) {
    let mut heartbeat = interval(HEARTBEAT);
    let mut access = interval(ACCESS_FLUSH);
    let mut usage = interval(USAGE_FLUSH);
    for ticker in [&mut heartbeat, &mut access, &mut usage] {
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    }
    loop {
        tokio::select! {
            _ = heartbeat.tick() => { job("heartbeat", app.engine.heartbeat()).await; }
            _ = access.tick() => { job("access_flush", app.engine.flush_access_days()).await; }
            _ = usage.tick() => { job("usage_flush", app.flush_usage()).await; }
            _ = stop.changed() => return,
        }
    }
}

async fn pack_totals(app: &App<Prod>) -> oxabl_cache_core::Result<(u64, u64, u64, u64)> {
    app.engine
        .metadata()
        .transact(|tx| {
            let mut sums = (0, 0, 0, 0);
            for cache in tx.all_caches()? {
                let totals = tx.pack_totals(&cache.id)?;
                sums.0 += totals.live_bytes;
                sums.1 += totals.pending_bytes;
                sums.2 += totals.reclaimable_bytes;
                sums.3 += totals.stored_bytes;
            }
            Ok(sums)
        })
        .await
}

async fn publish_pack_bytes(app: &App<Prod>) {
    if let Some((live, pending, reclaimable, stored)) = job("pack_totals", pack_totals(app)).await {
        for (state, bytes) in [
            ("live", live),
            ("pending", pending),
            ("reclaimable", reclaimable),
            ("stored", stored),
        ] {
            app.metrics
                .set("oxabl_cache_pack_bytes", &[("state", state)], bytes as f64);
        }
    }
}

async fn gc_tick(app: &App<Prod>) {
    let retired = job("retention", app.engine.gc_retention()).await;
    let evicted = job("eviction", app.engine.gc_eviction()).await;
    job("mark", app.engine.gc_mark()).await;
    let compacted = job("compaction", app.engine.gc_compaction()).await;
    let swept = job("sweep", app.engine.gc_sweep()).await;
    job("queued_scrubs", app.engine.run_queued_scrubs()).await;
    job("close_ledger_days", app.engine.close_ledger_days()).await;
    job("snapshot_stored_bytes", app.engine.snapshot_stored_bytes()).await;

    if let Some(packs) = swept {
        app.metrics
            .inc("oxabl_cache_gc_swept_packs_total", &[], packs);
    }
    if let Some(report) = compacted {
        app.metrics
            .inc("oxabl_cache_gc_compacted_bytes_total", &[], report.bytes);
    }
    let now = app.engine.clock().now();
    app.metrics
        .set("oxabl_cache_gc_last_run_timestamp", &[], now as f64);
    publish_pack_bytes(app).await;
    let changed = [
        retired.unwrap_or(0),
        evicted.unwrap_or(0),
        swept.unwrap_or(0),
        compacted.map_or(0, |report| report.packs),
    ];
    if changed.iter().any(|count| *count > 0) {
        emit(
            Level::Info,
            "gc",
            vec![
                ("retired_entries", Value::from(changed[0])),
                ("evicted_entries", Value::from(changed[1])),
                ("swept_packs", Value::from(changed[2])),
                ("compacted_packs", Value::from(changed[3])),
            ],
        );
    }
}

async fn weekly(app: &App<Prod>) {
    job("orphan_scan", app.engine.gc_orphans()).await;
    let caches = job("list_caches", async {
        app.engine
            .metadata()
            .transact(|tx| {
                Ok(tx
                    .all_caches()?
                    .into_iter()
                    .map(|cache| cache.id)
                    .collect::<Vec<_>>())
            })
            .await
    })
    .await;
    for cache in caches.unwrap_or_default() {
        job("scrub", app.engine.scrub_cache(cache)).await;
    }
}

async fn lead(app: &App<Prod>, election: &mut Election) -> bool {
    let leads = match election.poll().await {
        Ok(leads) => leads,
        Err(error) => {
            emit(
                Level::Warn,
                "leader_election_failed",
                vec![("error", Value::from(error))],
            );
            false
        }
    };
    if leads != app.gc_leader() {
        emit(
            Level::Info,
            if leads {
                "gc_leader_acquired"
            } else {
                "gc_leader_lost"
            },
            Vec::new(),
        );
    }
    app.set_gc_leader(leads);
    app.metrics
        .set("oxabl_cache_gc_leader", &[], if leads { 1.0 } else { 0.0 });
    leads
}

async fn gc_loop(
    app: Arc<App<Prod>>,
    gc: Gc,
    mut election: Election,
    mut stop: watch::Receiver<bool>,
) {
    let mut election_tick = interval(ELECTION);
    election_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut ticker = interval_at(tokio::time::Instant::now() + gc.interval, gc.interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_weekly = Instant::now();
    election_tick.tick().await;
    loop {
        tokio::select! {
            _ = election_tick.tick() => {
                let was = app.gc_leader();
                if lead(&app, &mut election).await && !was {
                    ticker.reset();
                    last_weekly = Instant::now();
                }
            }
            _ = ticker.tick() => {
                if !app.gc_leader() || !lead(&app, &mut election).await {
                    continue;
                }
                gc_tick(&app).await;
                if last_weekly.elapsed() >= gc.orphan_scan {
                    weekly(&app).await;
                    last_weekly = Instant::now();
                }
            }
            _ = stop.changed() => break,
        }
    }
    app.set_gc_leader(false);
    app.metrics.set("oxabl_cache_gc_leader", &[], 0.0);
}

/// Starts the background tasks; they end when `stop` changes.
pub(crate) async fn spawn(
    app: &Arc<App<Prod>>,
    gc: Gc,
    mut election: Election,
    stop: &watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    lead(app, &mut election).await;
    vec![
        tokio::spawn(flush_loop(Arc::clone(app), stop.clone())),
        tokio::spawn(gc_loop(Arc::clone(app), gc, election, stop.clone())),
    ]
}

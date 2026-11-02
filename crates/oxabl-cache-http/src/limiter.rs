//! Per-tenant request rate limiting: a token bucket per node, sized from the
//! tenant's rate divided by the live node count and refreshed every 10 seconds.

use crate::app::{App, Backend};
use oxabl_cache_core::Error;
use oxabl_cache_core::ids::TenantId;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct Bucket {
    rate: f64,
    burst: f64,
    tokens: f64,
    filled_at: Instant,
    sized_at: Instant,
}

/// The buckets of every tenant seen by this node.
#[derive(Debug, Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<TenantId, Bucket>>,
}

impl RateLimiter {
    fn stale(&self, tenant: &TenantId, now: Instant) -> bool {
        let buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        buckets
            .get(tenant)
            .is_none_or(|bucket| now.duration_since(bucket.sized_at) >= REFRESH)
    }

    fn resize(&self, tenant: TenantId, rate: f64, burst: f64, now: Instant) {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let bucket = buckets.entry(tenant).or_insert_with(|| Bucket {
            rate,
            burst,
            tokens: burst,
            filled_at: now,
            sized_at: now,
        });
        bucket.rate = rate;
        bucket.burst = burst;
        bucket.tokens = bucket.tokens.min(burst);
        bucket.sized_at = now;
    }

    fn take(&self, tenant: &TenantId, now: Instant) -> Result<(), u64> {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let Some(bucket) = buckets.get_mut(tenant) else {
            return Ok(());
        };
        let elapsed = now.duration_since(bucket.filled_at).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * bucket.rate).min(bucket.burst);
        bucket.filled_at = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            return Ok(());
        }
        let wait = (1.0 - bucket.tokens) / bucket.rate;
        Err(wait.ceil().max(1.0) as u64)
    }

    /// Admits one request of `tenant`, or reports how long to wait.
    pub(crate) async fn admit<B: Backend>(
        &self,
        app: &App<B>,
        tenant: TenantId,
    ) -> Result<(), Error> {
        let now = Instant::now();
        if self.stale(&tenant, now) {
            let found = app.engine.get_tenant(tenant).await?;
            let nodes = f64::from(app.engine.live_node_count().await.unwrap_or(1).max(1));
            let rate = (f64::from(found.rate) / nodes).max(0.1);
            let burst = (f64::from(found.burst) / nodes).max(1.0);
            self.resize(tenant, rate, burst, now);
        }
        self.take(&tenant, now)
            .map_err(|retry_after_secs| Error::RateLimited { retry_after_secs })
    }
}

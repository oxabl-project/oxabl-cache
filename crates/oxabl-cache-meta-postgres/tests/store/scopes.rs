//! Which transactions may overlap: writers of one cache never do, writers of
//! different caches do.

use crate::support::fresh;
use oxabl_cache_core::ids::{CacheId, TenantId};
use oxabl_cache_core::ports::{MetadataStore, TxScope};
use oxabl_cache_meta_postgres::PostgresStore;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const HOLD: Duration = Duration::from_millis(400);
const PATIENCE: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Meeting {
    inside: AtomicUsize,
    most: AtomicUsize,
}

impl Meeting {
    fn enter(&self) {
        let now = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
    }

    fn leave(&self) {
        self.inside.fetch_sub(1, Ordering::SeqCst);
    }

    /// Holds the transaction open: until the other one arrives when `overlap`
    /// is expected, else for a fixed time in which the other must not enter.
    fn hold(&self, overlap: bool) {
        if overlap {
            let deadline = Instant::now() + PATIENCE;
            while self.most.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        } else {
            std::thread::sleep(HOLD);
        }
    }
}

async fn most_at_once(store: &PostgresStore, a: TxScope, b: TxScope, overlap: bool) -> usize {
    let meeting = Arc::new(Meeting::default());
    let run = |scope: TxScope| {
        let (store, meeting) = (store.clone(), Arc::clone(&meeting));
        tokio::spawn(async move {
            store
                .transact_in(scope, move |_| {
                    meeting.enter();
                    meeting.hold(overlap);
                    meeting.leave();
                    Ok(())
                })
                .await
                .expect("transaction")
        })
    };
    let (first, second) = (run(a), run(b));
    first.await.expect("first");
    second.await.expect("second");
    meeting.most.load(Ordering::SeqCst)
}

#[tokio::test(flavor = "multi_thread")]
async fn writers_of_one_cache_serialise_and_writers_of_two_caches_overlap() {
    let (store, _database) = fresh().await;
    let (c1, c2) = (CacheId::random(), CacheId::random());
    let (t1, t2) = (TenantId::random(), TenantId::random());

    let cases = [
        ("one cache", TxScope::Cache(c1), TxScope::Cache(c1), false),
        ("two caches", TxScope::Cache(c1), TxScope::Cache(c2), true),
        (
            "two caches of a tenant, admitted",
            TxScope::TenantCache(t1, c1),
            TxScope::TenantCache(t1, c2),
            false,
        ),
        (
            "an admission and another cache",
            TxScope::TenantCache(t1, c1),
            TxScope::Cache(c2),
            true,
        ),
        (
            "admissions of two tenants",
            TxScope::TenantCache(t1, c1),
            TxScope::TenantCache(t2, c2),
            true,
        ),
        (
            "one tenant",
            TxScope::Tenant(t1),
            TxScope::Tenant(t1),
            false,
        ),
        (
            "two tenants",
            TxScope::Tenant(t1),
            TxScope::Tenant(t2),
            true,
        ),
        ("the ledger twice", TxScope::Ledger, TxScope::Ledger, false),
        (
            "ledger and a cache",
            TxScope::Ledger,
            TxScope::Cache(c1),
            true,
        ),
        (
            "independent rows",
            TxScope::Independent,
            TxScope::Independent,
            true,
        ),
        (
            "global and a cache",
            TxScope::Global,
            TxScope::Cache(c1),
            false,
        ),
        ("global and global", TxScope::Global, TxScope::Global, false),
        (
            "global and independent",
            TxScope::Global,
            TxScope::Independent,
            false,
        ),
    ];
    for (name, a, b, overlap) in cases {
        let most = most_at_once(&store, a, b, overlap).await;
        assert_eq!(
            most,
            if overlap { 2 } else { 1 },
            "{name}: {} at once",
            if overlap {
                "never overlapped"
            } else {
                "overlapped"
            }
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_runs_beside_a_global_writer() {
    let (store, _database) = fresh().await;
    let meeting = Arc::new(Meeting::default());
    let writer = {
        let (store, meeting) = (store.clone(), Arc::clone(&meeting));
        tokio::spawn(async move {
            store
                .transact_in(TxScope::Global, move |_| {
                    meeting.enter();
                    meeting.hold(true);
                    meeting.leave();
                    Ok(())
                })
                .await
                .expect("writer")
        })
    };
    let reader = {
        let (store, meeting) = (store.clone(), Arc::clone(&meeting));
        tokio::spawn(async move {
            store
                .read(move |_| {
                    meeting.enter();
                    meeting.hold(true);
                    meeting.leave();
                    Ok(())
                })
                .await
                .expect("reader")
        })
    };
    writer.await.expect("writer task");
    reader.await.expect("reader task");
    assert_eq!(meeting.most.load(Ordering::SeqCst), 2, "reads take no lock");
}

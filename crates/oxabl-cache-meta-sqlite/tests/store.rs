//! The SQLite store through the metadata port: durability, rollback, guards and migrations.

use oxabl_cache_core::Error;
use oxabl_cache_core::domain::cache::Cache;
use oxabl_cache_core::domain::pack::{Pack, PackState};
use oxabl_cache_core::domain::segment::{Segment, SegmentKind};
use oxabl_cache_core::domain::tenant::{Tenant, TenantStatus};
use oxabl_cache_core::ids::{CacheId, ClientRef, NodeId, PackId, PrincipalId, SegmentId, TenantId};
use oxabl_cache_core::ports::MetadataStore;
use oxabl_cache_meta_sqlite::SqliteStore;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(NodeId::random().to_string());
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn db(&self) -> PathBuf {
        self.0.join("meta.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tenant() -> Tenant {
    Tenant {
        id: TenantId::random(),
        name: "acme".into(),
        status: TenantStatus::Active,
        hard_bytes: None,
        egress_bytes: None,
        rate: 50,
        burst: 200,
        created_at: 1,
    }
}

fn cache(tenant: TenantId) -> Cache {
    Cache {
        id: CacheId::random(),
        tenant,
        display_name: "c".into(),
        retention_days: 30,
        hard_bytes: 1000,
        epoch: 0,
        roster_version: 0,
        live_bytes: 0,
        pending_bytes: 0,
        created_at: 1,
    }
}

fn pack(cache: CacheId) -> Pack {
    Pack {
        cache,
        id: PackId::random(),
        namespace: [0; 16],
        uploader: PrincipalId::random(),
        client_ref: ClientRef::random(),
        size: 100,
        etag: None,
        live_bytes: 0,
        state: PackState::Pending,
        lease_until: 10,
        created_at: 1,
        dead_since: None,
        commit_digest: None,
        commit_statuses: None,
    }
}

#[tokio::test]
async fn committed_work_survives_reopening_and_failed_work_leaves_no_trace() {
    let dir = Scratch::new();
    let store = SqliteStore::open(&dir.db()).expect("open");
    let kept = tenant();
    let discarded = tenant();
    let (kept_row, discarded_row) = (kept.clone(), discarded.clone());
    store
        .transact(move |tx| {
            tx.insert_tenant(&kept_row)?;
            Ok(())
        })
        .await
        .expect("commit");
    let failed: Result<(), Error> = store
        .transact(move |tx| {
            tx.insert_tenant(&discarded_row)?;
            Err(Error::NotFound)
        })
        .await;
    assert_eq!(failed, Err(Error::NotFound));
    drop(store);

    let reopened = SqliteStore::open(&dir.db()).expect("reopen");
    let (kept_id, discarded_id) = (kept.id, discarded.id);
    let found = reopened
        .transact(move |tx| Ok((tx.tenant(&kept_id)?, tx.tenant(&discarded_id)?)))
        .await
        .expect("read");
    assert_eq!(found.0, Some(kept));
    assert_eq!(
        found.1, None,
        "a transaction that returned an error was rolled back"
    );
}

#[tokio::test]
async fn a_panic_inside_a_transaction_rolls_it_back_and_the_store_keeps_working() {
    let dir = Scratch::new();
    let store = SqliteStore::open(&dir.db()).expect("open");
    let doomed = tenant();
    let doomed_row = doomed.clone();
    let crashed = store
        .transact(move |tx| -> Result<(), Error> {
            tx.insert_tenant(&doomed_row)?;
            panic!("worker dies mid-transaction")
        })
        .await;
    assert!(
        matches!(crashed, Err(Error::Internal(_))),
        "the panic is reported, not swallowed"
    );
    let doomed_id = doomed.id;
    let present = store
        .transact(move |tx| tx.tenant(&doomed_id))
        .await
        .expect("the store still works");
    assert_eq!(present, None);
}

#[tokio::test]
async fn the_schema_refuses_rows_that_break_its_invariants() {
    let dir = Scratch::new();
    let store = SqliteStore::open(&dir.db()).expect("open");
    let owner = tenant();
    let owned = cache(owner.id);
    let row = pack(owned.id);
    let (owner_row, owned_row, pack_row) = (owner.clone(), owned.clone(), row.clone());
    store
        .transact(move |tx| {
            tx.insert_tenant(&owner_row)?;
            tx.insert_cache(&owned_row)?;
            tx.insert_pack(&pack_row)
        })
        .await
        .expect("setup");

    let segment = |pack: PackId| Segment {
        cache: owned.id,
        id: SegmentId::random(),
        pack,
        offset: 24,
        length: 10,
        kind: SegmentKind::Artifact,
        created_at: 1,
        dead_since: None,
    };
    let dangling = segment(PackId::random());
    let refused = store.transact(move |tx| tx.insert_segment(&dangling)).await;
    assert!(
        matches!(refused, Err(Error::Internal(_))),
        "a segment must name an existing pack: {refused:?}"
    );

    let (cache_id, pack_id) = (owned.id, row.id);
    let overdrawn = store
        .transact(move |tx| tx.adjust_pack_live(&cache_id, &pack_id, 101))
        .await;
    assert!(
        matches!(overdrawn, Err(Error::Internal(_))),
        "live bytes cannot exceed the pack size"
    );
    let negative = store
        .transact(move |tx| tx.adjust_cache_bytes(&cache_id, -1, 0))
        .await;
    assert!(
        matches!(negative, Err(Error::Internal(_))),
        "byte counters cannot go negative"
    );

    let twin = Pack {
        id: PackId::random(),
        ..row.clone()
    };
    let duplicate = store.transact(move |tx| tx.insert_pack(&twin)).await;
    assert!(
        matches!(duplicate, Err(Error::Internal(_))),
        "one reservation per client_ref"
    );
}

#[tokio::test]
async fn a_database_written_by_a_newer_release_is_refused() {
    let dir = Scratch::new();
    drop(SqliteStore::open(&dir.db()).expect("open"));
    let raw = rusqlite::Connection::open(dir.db()).expect("raw");
    raw.execute(
        "INSERT INTO schema_migrations (version, applied_at) VALUES (999, 0)",
        [],
    )
    .expect("pretend");
    drop(raw);
    let refused = SqliteStore::open(&dir.db());
    assert!(matches!(refused, Err(Error::Unavailable(message)) if message.contains("newer")));
}

#[tokio::test]
async fn the_database_runs_in_wal_mode() {
    let dir = Scratch::new();
    let _store = SqliteStore::open(&dir.db()).expect("open");
    let raw = rusqlite::Connection::open(dir.db()).expect("raw");
    let mode: String = raw
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("mode");
    assert_eq!(mode, "wal");
}

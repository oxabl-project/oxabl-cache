//! A process that dies loses only its memory: the files decide what exists.

use crate::support::env::{Env, open_engine, small_config};
use crate::support::{entry, lookup_id};
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::entry::EntryStatus::Committed;
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;
use oxabl_cache_core::ids::PackId;
use oxabl_cache_core::ports::{ObjectKey, ObjectStore};
use oxabl_cache_core::usecases::{LookupRequest, NamespaceSet, UploadOutcome};

#[tokio::test]
async fn a_crash_between_upload_and_commit_leaves_nothing_an_entry_names() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 4096);
    let UploadOutcome::Reserved { pack, url } = env
        .reserve(&env.ci, Trusted, &built)
        .await
        .expect("reserve")
    else {
        panic!("expected a reservation");
    };
    env.put_through_url(&url, pack, &built.bytes)
        .expect("the bytes are durable");
    let Env {
        engine,
        meta,
        dir,
        clock,
        ci,
        cache,
        reader,
        ..
    } = env;
    drop(engine);
    drop(meta);

    let (restarted, _) = open_engine(dir.path(), &clock, small_config());
    let lookup = restarted
        .lookup(
            &reader,
            LookupRequest {
                cache,
                namespaces: NamespaceSet {
                    trusted: true,
                    personal: true,
                },
                ids: vec![lookup_id(1), lookup_id(2)],
                records: true,
                urls: false,
            },
        )
        .await
        .expect("lookup");
    assert!(lookup.hits.is_empty(), "no entry names the uploaded pack");
    let info = restarted.cache_info(&reader, cache).await.expect("info");
    assert_eq!(info.quota.live_bytes, 0);
    assert_eq!(
        info.quota.pending_bytes,
        built.bytes.len() as u64,
        "the reservation survived the restart"
    );

    clock.advance(59 * 60);
    assert_eq!(
        restarted.gc_sweep().await.expect("sweep"),
        0,
        "the lease has not run out"
    );
    clock.advance(2 * 60);
    assert_eq!(
        restarted.gc_sweep().await.expect("sweep"),
        1,
        "the expired lease is reclaimed"
    );
    let key = ObjectKey { cache, pack };
    assert!(
        restarted
            .object_store()
            .head(&key)
            .await
            .expect("head")
            .is_none(),
        "its bytes are gone"
    );
    let info = restarted.cache_info(&reader, cache).await.expect("info");
    assert_eq!((info.quota.pending_bytes, info.quota.stored_bytes), (0, 0));
    let late = restarted
        .commit_pack(&ci, late_commit(cache, pack, &built))
        .await;
    assert_eq!(
        late,
        Err(Error::NotFound),
        "a commit after the reclaim finds nothing"
    );
}

fn late_commit(
    cache: oxabl_cache_core::ids::CacheId,
    pack: PackId,
    built: &crate::support::Built,
) -> oxabl_cache_core::usecases::CommitRequest {
    oxabl_cache_core::usecases::CommitRequest {
        cache,
        pack,
        kind: Trusted,
        digest: [1; 32],
        pack_size: built.bytes.len() as u64,
        segments: built.segments.clone(),
        entries: vec![entry(1, 0)],
    }
}

#[tokio::test]
async fn committed_entries_and_their_recorded_answers_survive_a_restart() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 4096);
    let entries = vec![entry(1, 0), entry(2, 1)];
    let (pack, statuses) = env.publish(&env.ci, Trusted, &built, entries.clone()).await;
    assert_eq!(statuses, vec![Committed; 2]);
    let request = env.request(pack, Trusted, &built, entries);
    let Env {
        engine,
        meta,
        dir,
        clock,
        ci,
        cache,
        reader,
        ..
    } = env;
    drop(engine);
    drop(meta);

    let (restarted, _) = open_engine(dir.path(), &clock, small_config());
    let lookup = restarted
        .lookup(
            &reader,
            LookupRequest {
                cache,
                namespaces: NamespaceSet {
                    trusted: true,
                    personal: false,
                },
                ids: vec![lookup_id(1), lookup_id(2)],
                records: false,
                urls: false,
            },
        )
        .await
        .expect("lookup");
    assert_eq!(lookup.hits.len(), 2);
    let repeat = restarted.commit_pack(&ci, request).await.expect("repeat");
    assert_eq!(repeat, vec![Committed; 2], "the recorded answer is durable");
}

#[tokio::test]
async fn the_orphan_scan_removes_objects_without_a_row_only_once_they_are_old_enough() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 4096);
    let (live, _) = env
        .publish(&env.ci, Trusted, &built, vec![entry(1, 0)])
        .await;
    let pending_built = env.artifacts(&[2], 4096);
    let pending = env.upload(&env.ci, Trusted, &pending_built).await;
    let orphan = PackId::random();
    env.engine
        .object_store()
        .put(
            &ObjectKey {
                cache: env.cache,
                pack: orphan,
            },
            vec![9; 100],
        )
        .await
        .expect("orphan object");

    assert_eq!(
        env.engine.gc_orphans().await.expect("scan"),
        0,
        "a young object may belong to a commit in flight"
    );
    env.clock.advance(3 * 3600);
    assert_eq!(
        env.engine.gc_orphans().await.expect("scan"),
        1,
        "only the object no row names"
    );
    assert!(!env.object_exists(orphan).await);
    assert!(env.object_exists(live).await);
    assert!(env.object_exists(pending).await, "a pending pack has a row");
}

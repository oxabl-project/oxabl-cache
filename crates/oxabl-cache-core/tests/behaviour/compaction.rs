//! Compaction copies live segments without keys; readers keep their bytes through the grace.

use crate::support::{Env, entry};
use oxabl_cache_core::Config;
use oxabl_cache_core::domain::entry::EntryStatus::Committed;
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;
use oxabl_cache_core::ids::SegmentId;
use oxabl_cache_core::ports::{Clock, ObjectKey, ObjectStore};
use oxabl_cache_core::usecases::LookupResponse;
use oxabl_cache_store_fs::UrlRequest;

const SEGMENT: usize = 4096;

fn config() -> Config {
    Config {
        compaction_min_pack_bytes: 1024,
        ..Config::default()
    }
}

async fn read_hit(
    env: &Env,
    response: &LookupResponse,
    hit: usize,
) -> Result<Vec<u8>, oxabl_cache_core::Error> {
    let location = response.hits[hit].location.expect("fetch mode");
    let pack = response.packs[location.pack_index].pack;
    env.engine
        .object_store()
        .read_range(
            &ObjectKey {
                cache: env.cache,
                pack,
            },
            location.offset,
            u64::from(location.length),
            None,
        )
        .await
}

async fn mostly_dead_pack(env: &Env) -> crate::support::Built {
    let built = env.artifacts(&(1..=10).collect::<Vec<_>>(), SEGMENT);
    let (_, statuses) = env
        .publish(
            &env.ci,
            Trusted,
            &built,
            vec![entry(1, 0), entry(2, 1), entry(3, 2)],
        )
        .await;
    assert_eq!(
        statuses,
        vec![Committed; 3],
        "seven segments no entry names are dead at birth"
    );
    built
}

#[tokio::test]
async fn compaction_moves_live_segments_and_the_old_pack_is_swept_after_the_grace() {
    let env = Env::with(config(), 1 << 30).await;
    let built = mostly_dead_pack(&env).await;
    assert_eq!(env.info().await.quota.live_bytes, 3 * SEGMENT as u64);
    env.clock.advance(25 * 3600);

    let before = env.fetch(&env.reader, &[1, 2, 3], true).await;
    let old_pack = before.packs[0].pack;
    let report = env.engine.gc_compaction().await.expect("compaction");
    assert_eq!((report.packs, report.bytes), (1, 3 * SEGMENT as u64));

    let after = env.fetch(&env.reader, &[1, 2, 3], true).await;
    let new_pack = after.packs[0].pack;
    assert_ne!(new_pack, old_pack, "the segments live in a new pack now");
    for hit in 0..3 {
        let bytes = read_hit(&env, &after, hit)
            .await
            .expect("read from the new pack");
        assert_eq!(
            SegmentId::of_bytes(&bytes),
            built.segments[hit].id,
            "segment {hit} arrived intact"
        );
        assert_eq!(
            after.hits[hit].record, before.hits[hit].record,
            "records never change"
        );
    }
    assert_eq!(
        env.info().await.quota.live_bytes,
        3 * SEGMENT as u64,
        "live bytes are unchanged"
    );
    env.assert_consistent().await;

    assert!(
        env.object_exists(old_pack).await,
        "the old pack stays through the grace"
    );
    assert_eq!(env.engine.gc_sweep().await.expect("sweep"), 0);
    assert_eq!(
        env.engine.gc_compaction().await.expect("again").packs,
        0,
        "the new pack is fully live"
    );

    env.clock.advance(25 * 3600);
    assert_eq!(env.engine.gc_sweep().await.expect("sweep"), 1);
    assert!(!env.object_exists(old_pack).await);
    assert!(env.object_exists(new_pack).await);
    let bytes = read_hit(&env, &after, 0).await.expect("still readable");
    assert_eq!(SegmentId::of_bytes(&bytes), built.segments[0].id);
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_reader_with_a_url_resolved_before_compaction_still_reads_within_the_url_lifetime() {
    let env = Env::with(config(), 1 << 30).await;
    let built = mostly_dead_pack(&env).await;
    env.clock.advance(25 * 3600);

    let resolved = env.fetch(&env.reader, &[1, 2, 3], true).await;
    let url = env
        .engine
        .refresh_urls(&env.reader, env.cache, vec![resolved.packs[0].pack])
        .await
        .expect("urls");
    let (pack, signed) = &url[0];
    env.engine.gc_compaction().await.expect("compaction");
    env.clock.advance(5 * 60);

    let expires = signed
        .url
        .split("exp=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .and_then(|v| v.parse().ok())
        .expect("exp");
    let signature = signed.url.split("sig=").nth(1).expect("sig");
    env.engine
        .object_store()
        .verify_url(
            &UrlRequest {
                method: "GET",
                cache: env.cache,
                pack: *pack,
                expires_at: expires,
                content_length: None,
                signature,
            },
            env.engine.clock().now(),
        )
        .expect("the URL is still valid");
    for hit in 0..3 {
        let bytes = read_hit(&env, &resolved, hit)
            .await
            .expect("the old location still serves its bytes");
        assert_eq!(SegmentId::of_bytes(&bytes), built.segments[hit].id);
    }
}

#[tokio::test]
async fn compaction_never_copies_bytes_that_changed_and_the_scrub_then_removes_the_damaged_entry() {
    let env = Env::with(config(), 1 << 30).await;
    let built = mostly_dead_pack(&env).await;
    env.clock.advance(25 * 3600);
    let pack = env.fetch(&env.reader, &[1], true).await.packs[0].pack;

    let path = env.object_path(pack);
    let mut bytes = std::fs::read(&path).expect("read");
    bytes[usize::try_from(built.segments[1].offset).expect("offset") + 3] ^= 0x55;
    std::fs::write(&path, &bytes).expect("damage");

    let report = env.engine.gc_compaction().await.expect("compaction");
    assert_eq!(report.packs, 0, "a pack whose ETag changed is not copied");
    let scrub = env.engine.run_queued_scrubs().await.expect("scrub");
    assert_eq!((scrub.corrupt_segments, scrub.entries_removed), (1, 1));
    assert_eq!(env.hits(&env.reader, &[1, 2, 3], false).await, vec![0, 2]);

    let report = env.engine.gc_compaction().await.expect("compaction");
    assert_eq!(
        (report.packs, report.bytes),
        (1, 2 * SEGMENT as u64),
        "the intact segments move"
    );
    env.assert_consistent().await;
}

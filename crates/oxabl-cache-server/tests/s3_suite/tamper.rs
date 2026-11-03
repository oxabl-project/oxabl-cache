//! An object overwritten at rest through the S3 API is caught on read and by a report.

use crate::support::fixture::{World, hex};
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;
use serde_json::json;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

fn hits(world: &World, key: [u8; 32]) -> usize {
    let (bytes, shape) = world.fetch(&world.reader, &[key]);
    format::LookupResponse::decode(&bytes, &shape)
        .expect("OXLR")
        .hits
        .len()
}

#[test]
fn a_tampered_segment_fails_its_hash_on_read_and_a_report_confirms_it() {
    let world = World::start("s3-tamper", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000, 5_000]);
    let (a, b) = (frames::lookup_id(), frames::lookup_id());
    let (pack_id, _) = world.publish(
        &world.ci,
        TRUSTED,
        &pack,
        &[Entry::new(a, 0), Entry::new(b, 1)],
    );

    let read = |key: [u8; 32], segment: usize| {
        let (bytes, shape) = world.fetch(&world.reader, &[key]);
        let found = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
        let located = found.hits[0].record.as_ref().expect("record");
        let (offset, length) = (located.offset, u64::from(located.length));
        let range = format!("bytes={offset}-{}", offset + length - 1);
        let served = world.get_object(found.packs[0].url, Some(&range));
        assert_eq!(served.status, 206);
        served.body == pack.contents[segment]
    };
    assert!(read(b, 1), "the stored segment is intact");

    let flipped = usize::try_from(pack.segments[1].offset).expect("offset") + 100;
    world.flip_stored_bit(&pack_id, flipped, 0xff);
    assert!(
        !read(b, 1),
        "the tampered bytes differ from what was stored"
    );
    assert!(read(a, 0), "the other segment is untouched");

    let report = world.reader.json(
        "POST",
        &world.path("/reports"),
        Some(&json!({
            "pack_id": pack_id,
            "segment_id": hex(&pack.segments[1].segment_id.0),
            "kind": "hash_mismatch",
        })),
    );
    assert_eq!(report.status, 202);
    assert_eq!((hits(&world, a), hits(&world, b)), (1, 0));
    assert!(
        world
            .server
            .metrics_text()
            .contains("oxabl_cache_reports_total{outcome=\"confirmed\"} 1"),
        "the report is confirmed against the stored bytes"
    );
}

#[test]
fn a_replaced_object_is_pack_changed_on_a_commit_retry() {
    let world = World::start("s3-replaced", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let key = frames::lookup_id();
    let entries = [Entry::new(key, 0)];
    let (pack_id, _) = world.publish(&world.ci, TRUSTED, &pack, &entries);
    let frame = frames::commit(TRUSTED, &pack, &entries);
    assert_eq!(world.commit(&world.ci, &pack_id, &frame).status, 200);

    world.flip_stored_bit(&pack_id, 40, 0x01);
    let repeat = world.commit(&world.ci, &pack_id, &frame);
    assert_eq!(
        (repeat.status, repeat.json()["error"]["reason"].as_str()),
        (409, Some("pack_changed"))
    );
}

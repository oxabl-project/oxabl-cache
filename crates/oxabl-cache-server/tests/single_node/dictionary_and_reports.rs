//! The dictionary pointer, corruption reports, scrubs and purges.

use crate::roster_rules::put_roster;
use crate::support::fixture::{World, owner, roster_bytes, token};
use crate::support::frames::{self, Entry};
use crate::support::http::request;
use crate::support::server::Options;
use ed25519_dalek::{Signer, SigningKey};
use oxabl_cache_format as format;
use serde_json::json;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

fn hits(world: &World, key: [u8; 32]) -> usize {
    let (frame, shape) = frames::lookup(&[key], false, false, true, false);
    let answer = world.reader.binary("POST", &world.path("/lookup"), &frame);
    format::LookupResponse::decode(&answer.body, &shape)
        .expect("OXLR")
        .hits
        .len()
}

fn pointer(
    world: &World,
    version: u64,
    segment: format::SegmentId,
    signer: &SigningKey,
) -> Vec<u8> {
    let unsigned = format::DictionaryPointer {
        cache_id: format::CacheId(world.cache_bytes),
        epoch: 1,
        version,
        dict_segment_id: segment,
        signer: signer.verifying_key().to_bytes(),
        signature: [0; 64],
    };
    let mut message = b"oxabl-cache v1 dictionary".to_vec();
    message.extend_from_slice(&unsigned.signed_bytes());
    format::DictionaryPointer {
        signature: signer.sign(&message).to_bytes(),
        ..unsigned
    }
    .encode()
}

fn put_pointer(world: &World, if_match: &str, body: &[u8]) -> u16 {
    request(
        &world.server.public,
        "PUT",
        &world.path("/dictionary"),
        &[
            ("Authorization", format!("Bearer {}", world.ci.token)),
            ("Content-Type", "application/vnd.oxabl-cache.v1".to_owned()),
            ("If-Match", if_match.to_owned()),
        ],
        body,
    )
    .status
}

#[test]
fn a_dictionary_is_published_in_two_steps_and_only_a_ci_or_owner_key_may_point_at_it() {
    let world = World::start("dictionary", Options::default());
    let ci_key = SigningKey::from_bytes(&[3; 32]);
    let cache_admin = token(
        &world.admin,
        "cache-admin",
        &["read", "cache:admin"],
        &[&world.cache],
    );
    let cache_admin = world
        .admin
        .with_token(cache_admin["token"].as_str().expect("token"));
    let roster = roster_bytes(
        &world,
        &[(format::Role::Ci, ci_key.verifying_key().to_bytes())],
        1,
        1,
        &owner(),
    );
    assert_eq!(
        put_roster(&world, &cache_admin, Some("\"v0\""), &roster),
        204
    );

    let mut dictionary = frames::pack(world.cache_bytes, &[2_000]);
    dictionary.segments[0].kind = format::SegmentKind::Dictionary;
    let (_, statuses) = world.publish(&world.ci, TRUSTED, &dictionary, &[]);
    assert!(statuses.is_empty(), "a dictionary pack carries no entries");
    let segment = dictionary.segments[0].segment_id;
    assert_eq!(
        world.info(&world.reader)["quota"]["live_bytes"],
        2_000,
        "the dictionary is credited at commit"
    );

    let missing = world.reader.json("GET", &world.path("/dictionary"), None);
    assert_eq!(missing.status, 404);
    let stranger = SigningKey::from_bytes(&[4; 32]);
    assert_eq!(
        put_pointer(&world, "\"v0\"", &pointer(&world, 1, segment, &stranger)),
        400,
        "not a roster key"
    );
    assert_eq!(
        put_pointer(&world, "\"v3\"", &pointer(&world, 1, segment, &ci_key)),
        412,
        "stale If-Match"
    );
    let unknown = format::SegmentId([9; 32]);
    assert_eq!(
        put_pointer(&world, "\"v0\"", &pointer(&world, 1, unknown, &ci_key)),
        400,
        "not a live dictionary"
    );
    let valid = pointer(&world, 1, segment, &ci_key);
    assert_eq!(put_pointer(&world, "\"v0\"", &valid), 204);
    let stored = world.reader.json("GET", &world.path("/dictionary"), None);
    assert_eq!((stored.status, stored.body.clone()), (200, valid));
    assert_eq!(stored.header("etag"), Some("\"v1\""));
    assert_eq!(world.info(&world.reader)["dictionary_version"], 1);
}

#[test]
fn a_reported_corrupt_segment_is_confirmed_by_the_server_and_only_its_entries_are_removed() {
    let world = World::start("report", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000, 5_000]);
    let (a, b) = (frames::lookup_id(), frames::lookup_id());
    let (pack_id, _) = world.publish(
        &world.ci,
        TRUSTED,
        &pack,
        &[Entry::new(a, 0), Entry::new(b, 1)],
    );
    let report = |segment: usize| {
        world.reader.json(
            "POST",
            &world.path("/reports"),
            Some(&json!({
                "pack_id": pack_id,
                "segment_id": crate::support::fixture::hex(&pack.segments[segment].segment_id.0),
                "kind": "hash_mismatch",
            })),
        )
    };

    assert_eq!(report(0).status, 202);
    assert_eq!(
        (hits(&world, a), hits(&world, b)),
        (1, 1),
        "an intact segment is not removed on a client's word"
    );
    let flipped = usize::try_from(pack.segments[1].offset).expect("offset") + 100;
    world.flip_stored_bit(&pack_id, flipped, 0xff);
    assert_eq!(report(1).status, 202);
    assert_eq!(
        (hits(&world, a), hits(&world, b)),
        (1, 0),
        "the confirmed corrupt segment's entry is gone"
    );
    let page = world.server.metrics_text();
    assert!(
        page.contains("oxabl_cache_reports_total{outcome=\"confirmed\"} 1"),
        "{page}"
    );
    assert!(
        page.contains("oxabl_cache_reports_total{outcome=\"unconfirmed\"} 1"),
        "{page}"
    );
    let malformed = world.reader.json(
        "POST",
        &world.path("/reports"),
        Some(&json!({"pack_id": "x", "segment_id": "y", "kind": "hash_mismatch"})),
    );
    assert_eq!(malformed.status, 400);

    let flipped = usize::try_from(pack.segments[0].offset).expect("offset") + 7;
    world.flip_stored_bit(&pack_id, flipped, 0xff);
    let scrub = world.server.admin_call(
        "POST",
        &format!("/admin/v1/caches/{}/scrub", world.cache),
        None,
    );
    assert_eq!(scrub.status, 200);
    assert_eq!(scrub.json()["corrupt_segments"], 1);
    assert_eq!(hits(&world, a), 0, "a scrub finds what nobody reported");
}

#[test]
fn a_cache_admin_purges_old_epochs_and_one_personal_namespace() {
    let world = World::start("purge", Options::default());
    let keep = frames::lookup_id();
    let pack = frames::pack(world.cache_bytes, &[3_000]);
    world.publish(&world.ci, TRUSTED, &pack, &[Entry::new(keep, 0)]);

    let dev = token(
        &world.admin,
        "dev-2",
        &["read", "write:personal"],
        &[&world.cache],
    );
    let dev_client = world
        .admin
        .with_token(dev["token"].as_str().expect("token"));
    let personal_key = frames::lookup_id();
    let personal = frames::pack(world.cache_bytes, &[3_100]);
    world.publish(
        &dev_client,
        format::Namespace::Personal,
        &personal,
        &[Entry::new(personal_key, 0)],
    );
    let (frame, shape) = frames::lookup(&[personal_key], false, false, false, true);
    let own = |client: &crate::support::server::Client| {
        let answer = client.binary("POST", &world.path("/lookup"), &frame);
        format::LookupResponse::decode(&answer.body, &shape)
            .expect("OXLR")
            .hits
            .len()
    };
    assert_eq!(own(&dev_client), 1);

    let admin = token(
        &world.admin,
        "cache-admin",
        &["read", "cache:admin"],
        &[&world.cache],
    );
    let admin = world
        .admin
        .with_token(admin["token"].as_str().expect("token"));
    let user = dev["principal"].as_str().expect("principal");
    let purged = admin.json(
        "POST",
        &world.path("/purge"),
        Some(&json!({"namespace": "personal", "user": user})),
    );
    assert_eq!(
        (purged.status, purged.json()["entries_marked"].clone()),
        (202, json!(1))
    );
    assert_eq!(own(&dev_client), 0);
    assert_eq!(hits(&world, keep), 1, "the trusted entry stays");

    let nothing = admin.json(
        "POST",
        &world.path("/purge"),
        Some(&json!({"below_epoch": 0})),
    );
    assert_eq!(nothing.json()["entries_marked"], 0);
    let old = admin.json(
        "POST",
        &world.path("/purge"),
        Some(&json!({"below_epoch": 1})),
    );
    assert_eq!(old.json()["entries_marked"], 1);
    assert_eq!(hits(&world, keep), 0);
    let both = admin.json(
        "POST",
        &world.path("/purge"),
        Some(&json!({"below_epoch": 1, "user": "me"})),
    );
    assert_eq!(both.status, 400);
}

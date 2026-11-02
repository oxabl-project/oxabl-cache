//! The roster and the dictionary pointer under their versioning rules.

use crate::support::fixture::{World, owner, token};
use crate::support::http::request;
use crate::support::server::{Client, Options};
use ed25519_dalek::{Signer, SigningKey};
use oxabl_cache_format as format;

fn roster_bytes(world: &World, version: u64, epoch: u32, signer: &SigningKey) -> Vec<u8> {
    let owner_key = owner().verifying_key().to_bytes();
    let members = vec![format::Member {
        label: "owner",
        role: format::Role::Owner,
        sign_keys: vec![owner_key],
        age_recipients: Vec::new(),
    }];
    let unsigned = format::Roster {
        cache_id: format::CacheId(world.cache_bytes),
        version,
        epoch,
        members: members.clone(),
        keyring: b"opaque age file",
        signatures: Vec::new(),
    };
    let mut message = b"oxabl-cache v1 roster".to_vec();
    message.extend_from_slice(&unsigned.signed_bytes().expect("signed bytes"));
    let signature = format::RosterSignature {
        owner_key: signer.verifying_key().to_bytes(),
        signature: signer.sign(&message).to_bytes(),
    };
    format::Roster {
        signatures: vec![signature],
        ..unsigned
    }
    .encode()
    .expect("roster")
}

fn put(world: &World, admin: &Client, if_match: Option<&str>, body: &[u8]) -> u16 {
    let mut headers = vec![
        ("Authorization", format!("Bearer {}", admin.token)),
        ("Content-Type", "application/vnd.oxabl-cache.v1".to_owned()),
    ];
    if let Some(version) = if_match {
        headers.push(("If-Match", version.to_owned()));
    }
    request(
        &world.server.public,
        "PUT",
        &world.path("/roster"),
        &headers,
        body,
    )
    .status
}

#[test]
fn the_first_roster_is_version_one_epoch_one_and_needs_an_owner_signature() {
    let world = World::start("roster", Options::default());
    let created = token(
        &world.admin,
        "cache-admin",
        &["read", "cache:admin"],
        &[&world.cache],
    );
    let cache_admin = world
        .admin
        .with_token(created["token"].as_str().expect("token"));

    let before = world.reader.json("GET", &world.path("/roster"), None);
    assert_eq!(
        (before.status, before.error_code().as_str()),
        (404, "not_found")
    );
    let before = world.reader.json("GET", &world.path("/dictionary"), None);
    assert_eq!(before.status, 404);

    assert_eq!(
        put(
            &world,
            &world.reader,
            Some("\"v0\""),
            &roster_bytes(&world, 1, 1, &owner())
        ),
        403
    );
    assert_eq!(
        put(
            &world,
            &cache_admin,
            Some("\"v0\""),
            &roster_bytes(&world, 1, 0, &owner())
        ),
        400,
        "epoch 0"
    );
    assert_eq!(
        put(
            &world,
            &cache_admin,
            Some("\"v0\""),
            &roster_bytes(&world, 1, 2, &owner())
        ),
        400,
        "epoch 2"
    );
    assert_eq!(
        put(
            &world,
            &cache_admin,
            None,
            &roster_bytes(&world, 1, 1, &owner())
        ),
        400,
        "no If-Match"
    );
    let stranger = SigningKey::from_bytes(&[9; 32]);
    assert_eq!(
        put(
            &world,
            &cache_admin,
            Some("\"v0\""),
            &roster_bytes(&world, 1, 1, &stranger)
        ),
        400,
        "not an owner"
    );
    assert_eq!(
        put(
            &world,
            &cache_admin,
            Some("\"v5\""),
            &roster_bytes(&world, 1, 1, &owner())
        ),
        412,
        "stale"
    );
    assert_eq!(put(&world, &cache_admin, Some("\"v0\""), b"garbage"), 400);

    let valid = roster_bytes(&world, 1, 1, &owner());
    assert_eq!(put(&world, &cache_admin, Some("\"v0\""), &valid), 204);
    let stored = world.reader.json("GET", &world.path("/roster"), None);
    assert_eq!(stored.status, 200);
    assert_eq!(stored.body, valid);
    assert_eq!(stored.header("etag"), Some("\"v1\""));
    let info = world.info(&world.reader);
    assert_eq!(
        (info["roster_version"].as_u64(), info["epoch"].as_u64()),
        (Some(1), Some(1))
    );

    assert_eq!(
        put(
            &world,
            &cache_admin,
            Some("\"v1\""),
            &roster_bytes(&world, 2, 2, &owner())
        ),
        204,
        "rotation"
    );
    assert_eq!(
        put(
            &world,
            &cache_admin,
            Some("\"v2\""),
            &roster_bytes(&world, 3, 4, &owner())
        ),
        400,
        "epoch jump"
    );
}

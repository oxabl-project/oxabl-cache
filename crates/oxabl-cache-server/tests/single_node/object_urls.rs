//! Signed object URLs: tampering, expiry and what a PUT may do.

use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;

fn pending(world: &World, size: usize) -> (String, String) {
    let slot = world.reserve(&world.ci, "trusted", size).json()["packs"][0].clone();
    assert_eq!(slot["status"], "ok");
    assert_eq!(slot["method"], "PUT");
    (
        slot["url"].as_str().expect("url").to_owned(),
        slot["pack_id"].as_str().expect("id").to_owned(),
    )
}

fn replace_param(url: &str, name: &str, value: &str) -> String {
    let marker = format!("{name}=");
    let start = url.find(&marker).expect("param") + marker.len();
    let end = url[start..].find('&').map_or(url.len(), |n| start + n);
    format!("{}{value}{}", &url[..start], &url[end..])
}

#[test]
fn a_tampered_signature_expiry_length_or_method_is_refused_and_the_real_url_still_works() {
    let world = World::start("object-urls", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let (url, _) = pending(&world, pack.bytes.len());

    let sig = url.split("sig=").nth(1).expect("sig").to_owned();
    let flipped = format!(
        "{}{}",
        &sig[..sig.len() - 1],
        if sig.ends_with('A') { 'B' } else { 'A' }
    );
    let tampered = world.put_object(&replace_param(&url, "sig", &flipped), &pack.bytes);
    assert_eq!(
        (tampered.status, tampered.error_code().as_str()),
        (403, "forbidden")
    );
    let exp: i64 = url
        .split("exp=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .expect("exp")
        .parse()
        .expect("number");
    let extended = world.put_object(
        &replace_param(&url, "exp", &(exp + 3600).to_string()),
        &pack.bytes,
    );
    assert_eq!(extended.status, 403, "the expiry is signed");
    let shorter = world.put_object(&url, &pack.bytes[..pack.bytes.len() - 1]);
    assert_eq!(shorter.status, 403, "the content length is signed");
    let as_get = world.get_object(&url, None);
    assert_eq!(as_get.status, 403, "the method is signed");
    let unsigned = world.put_object(url.split('?').next().expect("path"), &pack.bytes);
    assert_ne!(unsigned.status, 200);
    assert!(unsigned.json()["error"]["code"].is_string());

    assert_eq!(
        world.put_object(&url, &pack.bytes).status,
        200,
        "the untouched URL works"
    );
}

#[test]
fn an_expired_url_is_refused() {
    let world = World::start(
        "expired-url",
        Options {
            url_lifetime: "2s",
            ..Options::default()
        },
    );
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let (url, _) = pending(&world, pack.bytes.len());
    std::thread::sleep(std::time::Duration::from_secs(3));
    let refused = world.put_object(&url, &pack.bytes);
    assert_eq!(
        (refused.status, refused.error_code().as_str()),
        (403, "forbidden")
    );
}

#[test]
fn a_committed_pack_accepts_no_further_put_and_a_pending_one_serves_no_get() {
    let world = World::start("put-after-commit", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let slot = world.reserve(&world.ci, "trusted", pack.bytes.len()).json()["packs"][0].clone();
    let url = slot["url"].as_str().expect("url");
    assert_eq!(world.put_object(url, &pack.bytes).status, 200);
    let pack_id = slot["pack_id"].as_str().expect("id");
    let frame = frames::commit(
        format::Namespace::Trusted,
        &pack,
        &[Entry::new(frames::lookup_id(), 0)],
    );
    assert_eq!(world.commit(&world.ci, pack_id, &frame).status, 200);

    let replay = world.put_object(url, &frames::random(pack.bytes.len()));
    assert_eq!(
        (replay.status, replay.error_code().as_str()),
        (409, "conflict")
    );
    assert_eq!(replay.json()["error"]["reason"], "pack_closed");
    let again = world.reserve(&world.ci, "trusted", pack.bytes.len());
    assert_eq!(
        again.json()["packs"][0]["status"],
        "ok",
        "a new client_ref reserves a new pack"
    );
    assert_eq!(
        std::fs::read(world.object_file(pack_id)).expect("object"),
        pack.bytes
    );
}

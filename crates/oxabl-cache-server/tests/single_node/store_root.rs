//! `/readyz` and the local store's root directory.

use crate::support::fixture::{World, wait_until};
use crate::support::http::request;
use crate::support::objects::Objects;
use crate::support::server::Options;
use std::fs;
use std::time::Duration;

#[test]
fn readyz_is_503_naming_the_store_while_the_store_root_is_gone() {
    let world = World::start("health-store-root", Options::default());
    let Objects::Fs { root } = world.server.objects.clone() else {
        panic!("a local-store scenario ran against a bucket");
    };
    let readyz = || request(&world.server.metrics, "GET", "/readyz", &[], b"");
    assert!(
        wait_until(Duration::from_secs(30), || readyz().status == 200),
        "the server is ready once it has started"
    );

    fs::remove_dir_all(&root).expect("remove the store root");
    assert!(
        wait_until(Duration::from_secs(10), || readyz().status == 503),
        "readiness fails once the store root is gone"
    );
    let down = readyz();
    assert_eq!(down.error_code(), "unavailable");
    let message = down.json()["error"]["message"]
        .as_str()
        .expect("message")
        .to_owned();
    assert!(message.contains("store"), "{message}");
    assert!(!message.contains("metadata"), "{message}");
    assert_eq!(
        request(&world.server.metrics, "GET", "/livez", &[], b"").status,
        200,
        "the process stays live while its store is gone"
    );

    fs::create_dir_all(root.join("tmp")).expect("restore the store root");
    assert!(
        wait_until(Duration::from_secs(10), || readyz().status == 200),
        "readiness returns once the store root is back"
    );
}

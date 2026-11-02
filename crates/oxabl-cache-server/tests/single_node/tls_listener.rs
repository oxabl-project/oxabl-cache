//! TLS 1.3 from certificate files, and nothing older.

use crate::support::http::request;
use crate::support::server::{Options, Server};
use crate::support::tls;
use rustls::version::{TLS12, TLS13};
use serde_json::{Value, json};

fn call(
    server: &Server,
    addr: &str,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> crate::support::http::Response {
    let bytes = body.map(Value::to_string).unwrap_or_default();
    let cert = server.cert_pem.as_deref().expect("a TLS server");
    tls::request(
        addr,
        cert,
        &[&TLS13],
        method,
        path,
        &[
            ("Authorization", format!("Bearer {token}")),
            ("Content-Type", "application/json".to_owned()),
        ],
        bytes.as_bytes(),
    )
    .expect("TLS 1.3 request")
}

#[test]
fn the_listeners_speak_tls_1_3_only_and_object_urls_are_https() {
    let server = Server::start(
        "tls-listener",
        Options {
            tls: true,
            ..Options::default()
        },
    );
    assert!(
        server.public_url.starts_with("https://"),
        "{}",
        server.public_url
    );

    let tenant = call(
        &server,
        &server.admin,
        &server.operator,
        "POST",
        "/admin/v1/tenants",
        Some(&json!({"name": "acme"})),
    );
    assert_eq!(tenant.status, 201);
    let tenant = tenant.json()["id"].as_str().expect("id").to_owned();
    let admin = call(
        &server,
        &server.admin,
        &server.operator,
        "POST",
        &format!("/admin/v1/tenants/{tenant}/tokens"),
        Some(&json!({"name": "admin"})),
    );
    let admin = admin.json()["token"].as_str().expect("token").to_owned();
    let cache = call(
        &server,
        &server.public,
        &admin,
        "POST",
        "/v1/tenant/caches",
        Some(&json!({"display_name": "c", "owner_keys": ["ab".repeat(32)]})),
    );
    assert_eq!(cache.status, 201);
    let cache = cache.json()["id"].as_str().expect("id").to_owned();
    let writer = call(
        &server,
        &server.public,
        &admin,
        "POST",
        "/v1/tenant/tokens",
        Some(&json!({"name": "w", "scopes": ["read", "write:trusted"], "caches": [cache]})),
    );
    let writer = writer.json()["token"].as_str().expect("token").to_owned();
    let reserved = call(
        &server,
        &server.public,
        &writer,
        "POST",
        &format!("/v1/caches/{cache}/uploads"),
        Some(
            &json!({"namespace": "trusted", "packs": [{"client_ref": "ab".repeat(16), "size": 4096}]}),
        ),
    );
    assert_eq!(reserved.status, 200);
    let url = reserved.json()["packs"][0]["url"]
        .as_str()
        .expect("url")
        .to_owned();
    assert!(
        url.starts_with("https://"),
        "object URLs point at the TLS listener: {url}"
    );

    let cert = server.cert_pem.as_deref().expect("cert");
    let old = tls::request(
        &server.public,
        cert,
        &[&TLS12],
        "GET",
        "/v1/nothing",
        &[],
        b"",
    );
    assert!(old.is_err(), "a TLS 1.2 client cannot connect");
    let plain =
        std::panic::catch_unwind(|| request(&server.public, "GET", "/v1/nothing", &[], b""));
    assert!(
        plain.is_err(),
        "plaintext HTTP gets no HTTP answer from a TLS listener"
    );
}

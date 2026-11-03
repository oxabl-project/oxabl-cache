//! TLS with system roots plus an optional CA file.

use crate::support::admin::run_with;
use crate::support::server::Server;
use std::fs;

#[test]
fn a_private_ca_needs_the_ca_file() {
    let server = Server::start("op-tls", true);
    let ca = server.dir.join("ca.pem");
    fs::write(&ca, server.cert_pem.as_ref().expect("certificate")).expect("ca");

    let untrusted = run_with(&server, &server.operator_token_file, None, &["health"]);
    assert_eq!(untrusted.code(), 3, "{}", untrusted.stderr());
    let trusted = run_with(&server, &server.operator_token_file, Some(&ca), &["health"]);
    assert_eq!(trusted.code(), 0, "{}", trusted.stderr());

    let garbage = server.dir.join("garbage.pem");
    fs::write(&garbage, "not a certificate").expect("file");
    let bad = run_with(
        &server,
        &server.operator_token_file,
        Some(&garbage),
        &["health"],
    );
    assert_eq!(bad.code(), 2, "{}", bad.stderr());
    let missing = run_with(
        &server,
        &server.operator_token_file,
        Some(&server.dir.join("none.pem")),
        &["health"],
    );
    assert_eq!(missing.code(), 2);
}

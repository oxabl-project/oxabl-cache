use crate::support::{ADDRESSES, Env, PRIVATE, key};
use oxabl_cache_e2e::idp::keys::Alg;
use oxabl_cache_e2e::netns;
use std::time::{Duration, Instant};

const GENERIC: &str = "the issuer could not be verified";

async fn refused(env: &Env, issuer: &str) {
    let reply = env.register(issuer, "custom", &["sub"]).await;
    assert_eq!(
        reply.status, 400,
        "{issuer} must be refused: {}",
        reply.body
    );
    assert_eq!(
        reply.json()["error"]["message"],
        GENERIC,
        "{issuer}: the error says nothing about why"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn issuers_that_point_inside_are_refused_at_registration() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    env.idp.publish("/github", vec![key(Alg::Es256, "k")]);
    for issuer in [
        "https://127.0.0.1",
        "https://127.0.0.1/path",
        "https://[::1]/",
        "https://[::ffff:127.0.0.1]/",
        "https://0.0.0.0/",
        "https://10.0.0.5/",
        "https://172.16.0.1/",
        "https://192.168.1.1/",
        "https://100.64.0.1/",
        "https://169.254.169.254/latest",
        "https://[fd00:ec2::254]/",
        "https://[fe80::1]/",
        "https://private.test/github",
        "https://localhost/",
        "https://unknown.test/",
        "http://idp.test/github",
        "https://idp.test:8443/github",
        "https://user:secret@idp.test/github",
        "ftp://idp.test/github",
        "idp.test/github",
    ] {
        refused(&env, issuer).await;
    }
    assert_eq!(
        env.decoy_connections(),
        0,
        "the private listener saw no connection"
    );
    assert_eq!(
        env.idp.seen().len(),
        0,
        "the provider saw no request either"
    );

    let allowed = env
        .register("https://idp.test/github", "custom", &["sub"])
        .await;
    assert_eq!(
        allowed.status, 201,
        "the allowed range works: {}",
        allowed.body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redirects_are_checked_like_the_first_request() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let targets = [
        ("/to-private-name", "https://private.test/x".to_owned()),
        ("/to-private-ip", format!("https://{PRIVATE}/x")),
        ("/to-loopback", "https://127.0.0.1/x".to_owned()),
        ("/to-port", "https://idp.test:8443/x".to_owned()),
        ("/to-http", "http://idp.test/x".to_owned()),
        ("/to-metadata", "https://169.254.169.254/latest".to_owned()),
    ];
    for (prefix, location) in &targets {
        env.idp.publish(prefix, vec![key(Alg::Es256, "k")]);
        env.idp.redirect_discovery(prefix, location);
        refused(&env, &format!("https://idp.test{prefix}")).await;
        assert_eq!(
            env.idp.seen_paths(prefix),
            1,
            "{prefix}: the redirect was not followed"
        );
    }
    assert_eq!(
        env.decoy_connections(),
        0,
        "no redirect reached the private listener"
    );

    env.idp.publish("/loop", vec![key(Alg::Es256, "k")]);
    env.idp.redirect_discovery(
        "/loop",
        "https://idp.test/loop/.well-known/openid-configuration",
    );
    refused(&env, "https://idp.test/loop").await;
    assert_eq!(
        env.idp.seen_paths("/loop"),
        4,
        "the first request and three redirects, no more"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_operator_allowlist_may_register() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::with("issuer_allowlist = [\"https://idp.test/github\"]\n").await;
    env.idp.publish("/github", vec![key(Alg::Es256, "k")]);
    env.idp.publish("/gitlab", vec![key(Alg::Es256, "k")]);
    assert_eq!(
        env.register("https://idp.test/github", "github", &[])
            .await
            .status,
        201
    );
    refused(&env, "https://idp.test/gitlab").await;
    assert_eq!(
        env.idp.seen_paths("/gitlab"),
        0,
        "an unlisted issuer is never contacted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_connection_names_the_issuer_in_sni_and_host() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    env.idp.publish("/github", vec![key(Alg::Es256, "k")]);
    assert_eq!(
        env.register("https://idp.test/github", "github", &[])
            .await
            .status,
        201
    );
    let seen = env.idp.seen();
    assert_eq!(seen.len(), 2, "discovery and the key set");
    for request in seen {
        assert_eq!(request.sni.as_deref(), Some("idp.test"), "{request:?}");
        assert_eq!(request.host, "idp.test", "{request:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_oversized_key_set_is_refused() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    env.idp.publish("/big", vec![key(Alg::Es256, "k")]);
    env.idp.pad_jwks("/big", 100 * 1024);
    assert_eq!(
        env.register("https://idp.test/big", "github", &[])
            .await
            .status,
        201,
        "100 KiB is fine"
    );
    env.idp.publish("/huge", vec![key(Alg::Es256, "k")]);
    env.idp.pad_jwks("/huge", 300 * 1024);
    refused(&env, "https://idp.test/huge").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stalled_issuer_is_given_up_on_after_five_seconds() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    env.idp.publish("/slow", vec![key(Alg::Es256, "k")]);
    env.idp.stall_jwks("/slow");
    let started = Instant::now();
    refused(&env, "https://idp.test/slow").await;
    let took = started.elapsed();
    assert!(
        took >= Duration::from_secs(4),
        "it waited the budget: {took:?}"
    );
    assert!(took < Duration::from_secs(8), "and no longer: {took:?}");
}

//! `oxabl-cache-admin tenant bootstrap-admin` binds a human administrator who then signs in.

use crate::support::{Env, error_code, key, token_of};
use oxabl_cache_e2e::idp::keys::Alg;
use oxabl_cache_e2e::idp::tokens;
use oxabl_cache_e2e::netns;
use serde_json::Value;

use crate::support::ADDRESSES;

fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_admin_cli_binds_a_human_administrator_who_then_signs_in() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let signing = key(Alg::Es256, "k1");
    env.idp.publish("/people", vec![signing.clone()]);
    let issuer = "https://idp.test/people";
    let bind = |subject: &str, issuer: &str| {
        env.server.admin_cli(&[
            "--json",
            "tenant",
            "bootstrap-admin",
            &env.tenant.id,
            "--issuer",
            issuer,
            "--subject",
            subject,
        ])
    };

    let first = bind("alice", issuer);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let id = json(&first)["id"].as_str().expect("id").to_owned();
    let again = bind("alice", issuer);
    assert!(again.status.success());
    assert_eq!(
        json(&again)["id"],
        id,
        "the same subject is the same principal"
    );
    let refused = bind("bob", "https://private.test/x");
    assert_eq!(
        refused.status.code(),
        Some(1),
        "an issuer behind the guard is refused"
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("bad_request"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    let audience = env.tenant.audience();
    let alice = env
        .exchange(&tokens::sign(
            &signing,
            &tokens::generic(issuer, &audience, "alice"),
        ))
        .await;
    assert_eq!(alice.status, 200, "{}", alice.body);
    let caches = env
        .server
        .api(&token_of(&alice), "GET", "/v1/tenant/caches", None)
        .await;
    assert_eq!(caches.status, 200, "the bound human administers the tenant");
    let mallory = env
        .exchange(&tokens::sign(
            &signing,
            &tokens::generic(issuer, &audience, "mallory"),
        ))
        .await;
    assert_eq!(mallory.status, 401);
    assert_eq!(error_code(&mallory), "unauthenticated");
}

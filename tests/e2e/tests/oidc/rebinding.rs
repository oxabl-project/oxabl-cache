use crate::support::{ADDRESSES, Env, PRIVATE, PUBLIC, REPOSITORY, REPOSITORY_ID, key, token_of};
use oxabl_cache_e2e::idp::keys::Alg;
use oxabl_cache_e2e::idp::tokens;
use oxabl_cache_e2e::netns;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refetch_after_the_name_moved_inside_is_refused() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let first = key(Alg::Es256, "k1");
    env.idp.publish("/g", vec![first.clone()]);
    let issuer = "https://rebind.test/g";
    assert_eq!(env.register(issuer, "github", &[]).await.status, 201);
    let made = env
        .policy(
            "writer",
            issuer,
            serde_json::json!({"repository_id": REPOSITORY_ID}),
            &["read"],
        )
        .await;
    assert_eq!(made.status, 201, "{}", made.body);
    let claims = |ref_name: &str| {
        tokens::github(
            issuer,
            &env.tenant.audience(),
            REPOSITORY,
            REPOSITORY_ID,
            ref_name,
            "e",
        )
    };
    assert_eq!(
        env.exchange(&tokens::sign(&first, &claims("refs/heads/main")))
            .await
            .status,
        200
    );
    let before = env.idp.seen().len();

    env.dns.set("rebind.test", PRIVATE);
    let rotated = key(Alg::Es256, "k2");
    env.idp.publish("/g", vec![rotated.clone()]);
    tokio::time::sleep(Duration::from_millis(1300)).await;
    let token = tokens::sign(&rotated, &claims("refs/heads/main"));
    let refetch = env.exchange(&token).await;
    assert_ne!(
        refetch.status, 200,
        "the refetch resolved inside: {}",
        refetch.body
    );
    assert_eq!(
        env.decoy.connections(),
        0,
        "the private listener saw no connection"
    );
    assert_eq!(
        env.idp.seen().len(),
        before,
        "the provider saw nothing from the refetch"
    );

    env.dns.set("rebind.test", PUBLIC);
    let back = env.exchange(&token).await;
    assert_eq!(
        back.status, 200,
        "with the name back outside the same token works: {}",
        back.body
    );
    token_of(&back);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_first_fetch_and_a_registration_behind_a_moved_name_are_refused() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let signing = key(Alg::Es256, "k1");
    env.idp.publish("/g", vec![signing.clone()]);
    let issuer = "https://rebind.test/g";
    assert_eq!(env.register(issuer, "github", &[]).await.status, 201);
    let made = env
        .policy(
            "writer",
            issuer,
            serde_json::json!({"repository_id": REPOSITORY_ID}),
            &["read"],
        )
        .await;
    assert_eq!(made.status, 201, "{}", made.body);

    env.dns.set("rebind.test", PRIVATE);
    let token = tokens::sign(
        &signing,
        &tokens::github(
            issuer,
            &env.tenant.audience(),
            REPOSITORY,
            REPOSITORY_ID,
            "r",
            "e",
        ),
    );
    let reply = env.exchange(&token).await;
    assert_ne!(reply.status, 200, "{}", reply.body);
    let registered = env
        .register("https://rebind.test/other", "github", &[])
        .await;
    assert_eq!(registered.status, 400, "{}", registered.body);
    assert_eq!(env.decoy.connections(), 0);
    assert_eq!(
        env.idp.seen().len(),
        2,
        "only the registration's own two requests"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn answers_that_alternate_between_check_and_connect_never_reach_the_private_listener() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    env.idp.publish("/g", vec![key(Alg::Es256, "k1")]);
    env.dns.alternate("rebind.test", PUBLIC, PRIVATE);
    let mut accepted = 0;
    let mut refused = 0;
    for attempt in 0..12 {
        let issuer = format!("https://rebind.test/g?attempt={attempt}");
        let reply = env.register(&issuer, "github", &[]).await;
        match reply.status {
            201 | 200 => accepted += 1,
            400 => refused += 1,
            other => panic!("unexpected {other}: {}", reply.body),
        }
    }
    assert!(
        refused > 0,
        "the private answers were refused ({accepted} accepted)"
    );
    assert_eq!(
        env.decoy.connections(),
        0,
        "the private listener saw no connection"
    );
    for request in env.idp.seen() {
        assert_eq!(request.sni.as_deref(), Some("rebind.test"), "{request:?}");
    }
}

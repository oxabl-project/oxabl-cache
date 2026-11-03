use crate::support::{ADDRESSES, Env, REPOSITORY, REPOSITORY_ID, key, token_of};
use oxabl_cache_e2e::idp::keys::Alg;
use oxabl_cache_e2e::idp::tokens::{self, now};
use oxabl_cache_e2e::netns;
use serde_json::json;

fn uploads(namespace: &str) -> serde_json::Value {
    json!({"namespace": namespace, "packs": [{"client_ref": "ab".repeat(16), "size": 4096}]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_issuer_shape_gets_exactly_the_scopes_of_its_policy() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let cache = env.tenant.cache.clone();
    let audience = env.tenant.audience();

    let github_key = key(Alg::Rs256, "gh-1");
    let github = env.github_writer("/github", github_key.clone()).await;

    let gitlab_key = key(Alg::Es256, "gl-1");
    env.idp.publish("/gitlab", vec![gitlab_key.clone()]);
    let gitlab = "https://idp.test/gitlab";
    assert_eq!(env.register(gitlab, "gitlab", &[]).await.status, 201);
    let made = env
        .policy(
            "gitlab-reader",
            gitlab,
            json!({"project_id": "424242", "ref": "refs/heads/main"}),
            &["read"],
        )
        .await;
    assert_eq!(made.status, 201, "{}", made.body);

    let generic_key = key(Alg::EdDsa, "gen-1");
    env.idp.publish("/generic", vec![generic_key.clone()]);
    let generic = "https://idp.test/generic";
    assert_eq!(env.register(generic, "custom", &["sub"]).await.status, 201);
    let made = env
        .policy(
            "bot",
            generic,
            json!({"sub": "ci-bot"}),
            &["read", "write:personal"],
        )
        .await;
    assert_eq!(made.status, 201, "{}", made.body);

    let cases = [
        (
            "github",
            tokens::sign(
                &github_key,
                &tokens::github(
                    &github,
                    &audience,
                    REPOSITORY,
                    REPOSITORY_ID,
                    "refs/heads/main",
                    "cache-writer",
                ),
            ),
            json!(["read", "write:trusted"]),
        ),
        (
            "gitlab",
            tokens::sign(
                &gitlab_key,
                &tokens::gitlab(gitlab, &audience, "group/proj", "424242", "refs/heads/main"),
            ),
            json!(["read"]),
        ),
        (
            "generic",
            tokens::sign(&generic_key, &tokens::generic(generic, &audience, "ci-bot")),
            json!(["read", "write:personal"]),
        ),
    ];
    for (name, token, scopes) in cases {
        let reply = env.exchange(&token).await;
        assert_eq!(reply.status, 200, "{name}: {}", reply.body);
        let body = reply.json();
        assert_eq!(
            body["grants"],
            json!([{"cache": cache, "scopes": scopes}]),
            "{name} gets exactly its policy's scopes"
        );
        let session = token_of(&reply);
        assert!(session.starts_with("oxs_"), "{name}: a session token");
        let lifetime = body["expires_at"].as_i64().expect("expiry") - now();
        assert!(
            (3590..=3610).contains(&lifetime),
            "{name}: a workload session lasts 1 h, got {lifetime}"
        );

        let info = env
            .server
            .api(&session, "GET", &format!("/v1/caches/{cache}"), None)
            .await;
        assert_eq!(info.status, 200, "{name} can read");
        let trusted = env
            .server
            .api(
                &session,
                "POST",
                &format!("/v1/caches/{cache}/uploads"),
                Some(&uploads("trusted")),
            )
            .await;
        let personal = env
            .server
            .api(
                &session,
                "POST",
                &format!("/v1/caches/{cache}/uploads"),
                Some(&uploads("personal")),
            )
            .await;
        let can = |scope: &str| {
            scopes
                .as_array()
                .expect("scopes")
                .iter()
                .any(|s| s == scope)
        };
        assert_eq!(
            trusted.status == 200,
            can("write:trusted"),
            "{name} trusted: {}",
            trusted.body
        );
        assert_eq!(
            personal.status == 200,
            can("write:personal"),
            "{name} personal: {}",
            personal.body
        );
        let admin = env
            .server
            .api(&session, "GET", "/v1/tenant/tokens", None)
            .await;
        assert_eq!(admin.status, 403, "{name} never administers the tenant");
        let other = env
            .server
            .api(
                &session,
                "POST",
                "/v1/tenant/oidc/policies",
                Some(&json!({})),
            )
            .await;
        assert_eq!(other.status, 403, "{name} cannot write policies");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_ends_when_it_is_revoked() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let signing = key(Alg::Es256, "k1");
    let issuer = env.github_writer("/github", signing.clone()).await;
    let token = tokens::sign(
        &signing,
        &tokens::github(
            &issuer,
            &env.tenant.audience(),
            REPOSITORY,
            REPOSITORY_ID,
            "refs/heads/main",
            "cache-writer",
        ),
    );
    let session = token_of(&env.exchange(&token).await);
    let path = format!("/v1/caches/{}", env.tenant.cache);
    assert_eq!(
        env.server.api(&session, "GET", &path, None).await.status,
        200
    );
    assert_eq!(
        env.server
            .api(&session, "DELETE", "/v1/auth/session", None)
            .await
            .status,
        204
    );
    assert_eq!(
        env.server.api(&session, "GET", &path, None).await.status,
        401
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_human_with_a_registered_subject_gets_a_twelve_hour_admin_session() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let signing = key(Alg::Es256, "k1");
    env.idp.publish("/people", vec![signing.clone()]);
    let issuer = "https://idp.test/people";
    let route = format!("/admin/v1/tenants/{}/admins", env.tenant.id);
    let body = json!({"issuer": issuer, "subject": "alice"});

    let first = env.server.operator("POST", &route, Some(&body)).await;
    assert_eq!(first.status, 201, "{}", first.body);
    let again = env.server.operator("POST", &route, Some(&body)).await;
    assert_eq!(again.status, 200, "the same subject again");
    assert_eq!(again.json()["id"], first.json()["id"]);
    let refused = env
        .server
        .operator(
            "POST",
            &route,
            Some(&json!({"issuer": "https://private.test/x", "subject": "bob"})),
        )
        .await;
    assert_eq!(
        refused.status, 400,
        "an issuer behind the guard cannot be bound"
    );
    let not_operator = env
        .server
        .operator_as(&env.tenant.admin, "POST", &route, Some(&body))
        .await;
    assert_eq!(
        not_operator.status, 401,
        "a tenant token is no operator token"
    );

    let audience = env.tenant.audience();
    let alice = env
        .exchange(&tokens::sign(
            &signing,
            &tokens::generic(issuer, &audience, "alice"),
        ))
        .await;
    assert_eq!(alice.status, 200, "{}", alice.body);
    assert_eq!(
        alice.json()["grants"],
        json!([{"cache": null, "scopes": ["tenant:admin"]}])
    );
    let lifetime = alice.json()["expires_at"].as_i64().expect("expiry") - now();
    assert!(
        (43190..=43210).contains(&lifetime),
        "a human session lasts 12 h, got {lifetime}"
    );
    let caches = env
        .server
        .api(&token_of(&alice), "GET", "/v1/tenant/caches", None)
        .await;
    assert_eq!(caches.status, 200, "{}", caches.body);

    let mallory = env
        .exchange(&tokens::sign(
            &signing,
            &tokens::generic(issuer, &audience, "mallory"),
        ))
        .await;
    assert_eq!(
        mallory.status, 401,
        "a valid token of an unregistered subject"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_database_holds_no_claim_value_and_no_session_secret() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let signing = key(Alg::Es256, "k1");
    let issuer = env.github_writer("/github", signing.clone()).await;
    let token = tokens::sign(
        &signing,
        &tokens::github(
            &issuer,
            &env.tenant.audience(),
            REPOSITORY,
            REPOSITORY_ID,
            "refs/heads/main",
            "cache-writer",
        ),
    );
    let session = token_of(&env.exchange(&token).await);
    let database = env.server.database_bytes();
    let contains = |needle: &str| {
        database
            .windows(needle.len())
            .any(|w| w == needle.as_bytes())
    };
    assert!(
        contains("writer"),
        "the scan reads the database: the policy name is stored"
    );
    for value in [REPOSITORY_ID, "refs/heads/main", "cache-writer"] {
        assert!(!contains(value), "the claim value {value} is stored");
    }
    let secret = session.rsplit('_').next().expect("secret");
    assert!(!contains(secret), "the session secret is stored");
    assert!(!contains(&token), "the JWT is stored");
}

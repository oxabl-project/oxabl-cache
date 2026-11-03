use crate::support::{ADDRESSES, Env, REPOSITORY, REPOSITORY_ID, error_code, key, token_of};
use oxabl_cache_e2e::idp::keys::Alg;
use oxabl_cache_e2e::idp::tokens;
use oxabl_cache_e2e::netns;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_policy_must_pin_an_immutable_id_of_its_issuer() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    for prefix in ["/gh", "/gl", "/custom", "/uuid"] {
        env.idp.publish(prefix, vec![key(Alg::Es256, "k")]);
    }
    let github = "https://idp.test/gh";
    let gitlab = "https://idp.test/gl";
    let custom = "https://idp.test/custom";
    let uuid = "https://idp.test/uuid";
    assert_eq!(env.register(github, "github", &[]).await.status, 201);
    assert_eq!(env.register(gitlab, "gitlab", &[]).await.status, 201);
    assert_eq!(env.register(custom, "custom", &["sub"]).await.status, 201);
    assert_eq!(
        env.register(uuid, "custom", &["tenant_uuid"]).await.status,
        201
    );

    let refused = [
        (
            "github: environment only",
            github,
            json!({"environment": "cache-writer"}),
        ),
        (
            "github: repository name only",
            github,
            json!({"repository": REPOSITORY}),
        ),
        (
            "github: owner id alone",
            github,
            json!({"repository_owner_id": "5550001"}),
        ),
        (
            "github: ref and environment",
            github,
            json!({"ref": "refs/heads/main", "environment": "e"}),
        ),
        (
            "gitlab: ref only",
            gitlab,
            json!({"ref": "refs/heads/main"}),
        ),
        (
            "gitlab: namespace id alone",
            gitlab,
            json!({"namespace_id": "7"}),
        ),
        (
            "gitlab: project path only",
            gitlab,
            json!({"project_path": "g/p"}),
        ),
        (
            "custom with sub: environment only",
            custom,
            json!({"environment": "cache-writer"}),
        ),
        (
            "custom with a declared claim: sub only",
            uuid,
            json!({"sub": "x"}),
        ),
    ];
    for (name, issuer, claims) in refused {
        let reply = env.policy(name, issuer, claims, &["read"]).await;
        assert_eq!(reply.status, 400, "{name}: {}", reply.body);
    }
    let accepted = [
        (
            "github: repository id",
            github,
            json!({"repository_id": REPOSITORY_ID}),
        ),
        (
            "github: owner id and repository",
            github,
            json!({"repository_owner_id": "5550001", "repository": REPOSITORY}),
        ),
        ("gitlab: project id", gitlab, json!({"project_id": "42"})),
        (
            "gitlab: namespace id and project path",
            gitlab,
            json!({"namespace_id": "7", "project_path": "g/p"}),
        ),
        ("custom: sub", custom, json!({"sub": "bot"})),
        (
            "custom: declared claim",
            uuid,
            json!({"tenant_uuid": "u-1"}),
        ),
    ];
    for (name, issuer, claims) in accepted {
        let reply = env.policy(name, issuer, claims, &["read"]).await;
        assert_eq!(reply.status, 201, "{name}: {}", reply.body);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_workload_policy_never_grants_administration() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    env.idp.publish("/gh", vec![key(Alg::Es256, "k")]);
    let issuer = "https://idp.test/gh";
    assert_eq!(env.register(issuer, "github", &[]).await.status, 201);
    let claims = json!({"repository_id": REPOSITORY_ID});
    for scopes in [
        vec!["cache:admin"],
        vec!["tenant:admin"],
        vec!["read", "cache:admin"],
        vec!["write:trusted", "tenant:admin"],
        vec!["root"],
        vec![],
    ] {
        let reply = env.policy("admin", issuer, claims.clone(), &scopes).await;
        assert_eq!(reply.status, 400, "{scopes:?}: {}", reply.body);
    }
    let listed = env.admin("GET", "/v1/tenant/oidc/policies", None).await;
    assert_eq!(listed.json()["policies"], json!([]), "nothing was stored");
    let ok = env
        .policy(
            "writer",
            issuer,
            claims,
            &["read", "write:personal", "write:trusted"],
        )
        .await;
    assert_eq!(ok.status, 201, "{}", ok.body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn policies_and_issuers_use_natural_keys_and_hide_claim_values() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    env.idp.publish("/gh", vec![key(Alg::Es256, "k")]);
    let issuer = "https://idp.test/gh";
    assert_eq!(env.register(issuer, "github", &[]).await.status, 201);
    assert_eq!(
        env.register(issuer, "github", &[]).await.status,
        200,
        "the identical issuer again"
    );
    assert_eq!(
        env.register(issuer, "gitlab", &[]).await.status,
        409,
        "another shape"
    );
    assert_eq!(
        env.register("https://idp.test/gh2", "github", &["x"])
            .await
            .status,
        400
    );
    assert_eq!(
        env.register("https://idp.test/gh2", "custom", &[])
            .await
            .status,
        400
    );

    let claims = json!({"repository_id": REPOSITORY_ID});
    let first = env
        .policy("writer", issuer, claims.clone(), &["read"])
        .await;
    assert_eq!(first.status, 201);
    let again = env
        .policy("writer", issuer, claims.clone(), &["read"])
        .await;
    assert_eq!(again.status, 200);
    assert_eq!(again.json()["id"], first.json()["id"]);
    let different = env
        .policy("writer", issuer, claims.clone(), &["read", "write:trusted"])
        .await;
    assert_eq!(different.status, 409);
    let elsewhere = env
        .policy("other", "https://idp.test/unregistered", claims, &["read"])
        .await;
    assert_eq!(elsewhere.status, 400, "an unregistered issuer");

    let listed = env.admin("GET", "/v1/tenant/oidc/policies", None).await;
    assert_eq!(listed.status, 200);
    assert!(
        listed.body.contains("repository_id"),
        "claim names are listed"
    );
    assert!(
        !listed.body.contains(REPOSITORY_ID),
        "claim values are not: {}",
        listed.body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_a_policy_or_an_issuer_ends_its_sessions() {
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
    let path = format!("/v1/caches/{}", env.tenant.cache);

    let session = token_of(&env.exchange(&token).await);
    assert_eq!(
        env.server.api(&session, "GET", &path, None).await.status,
        200
    );
    let removed = env
        .admin(
            "DELETE",
            "/v1/tenant/oidc/policies",
            Some(&json!({"name": "writer"})),
        )
        .await;
    assert_eq!(removed.status, 204, "{}", removed.body);
    assert_eq!(
        env.server.api(&session, "GET", &path, None).await.status,
        401
    );
    let denied = env.exchange(&token).await;
    assert_eq!(denied.status, 401, "no policy matches any more");
    assert_eq!(error_code(&denied), "unauthenticated");

    let again = env
        .policy(
            "writer",
            &issuer,
            json!({"repository_id": REPOSITORY_ID, "ref": "refs/heads/main", "environment": "cache-writer"}),
            &["read"],
        )
        .await;
    assert_eq!(again.status, 201);
    let session = token_of(&env.exchange(&token).await);
    let removed = env
        .admin(
            "DELETE",
            "/v1/tenant/oidc/issuers",
            Some(&json!({"issuer": issuer})),
        )
        .await;
    assert_eq!(removed.status, 204, "{}", removed.body);
    assert_eq!(
        env.server.api(&session, "GET", &path, None).await.status,
        401
    );
    assert_eq!(
        env.exchange(&token).await.status,
        401,
        "the issuer is no longer trusted"
    );
    let listed = env.admin("GET", "/v1/tenant/oidc/policies", None).await;
    assert_eq!(
        listed.json()["policies"],
        json!([]),
        "the issuer's policies went with it"
    );
}

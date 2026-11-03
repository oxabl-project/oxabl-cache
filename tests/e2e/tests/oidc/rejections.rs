use crate::support::{ADDRESSES, Env, REPOSITORY, REPOSITORY_ID, error_code, key};
use oxabl_cache_e2e::idp::keys::{Alg, SigningKey};
use oxabl_cache_e2e::idp::tokens::{self, now};
use oxabl_cache_e2e::netns;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

struct Setup {
    env: Env,
    issuer: String,
    key: Arc<SigningKey>,
}

impl Setup {
    async fn start(alg: Alg) -> Self {
        let env = Env::start().await;
        let key = key(alg, "k1");
        let issuer = env.github_writer("/github", key.clone()).await;
        Self { env, issuer, key }
    }

    fn claims(&self) -> Value {
        tokens::github(
            &self.issuer,
            &self.env.tenant.audience(),
            REPOSITORY,
            REPOSITORY_ID,
            "refs/heads/main",
            "cache-writer",
        )
    }

    async fn status(&self, token: &str) -> u16 {
        self.env.exchange(token).await.status
    }

    async fn accepts(&self, claims: &Value) -> bool {
        self.status(&tokens::sign(&self.key, claims)).await == 200
    }

    async fn refuses(&self, what: &str, token: &str) {
        let reply = self.env.exchange(token).await;
        assert_eq!(reply.status, 401, "{what}: {}", reply.body);
        assert_eq!(
            error_code(&reply),
            "unauthenticated",
            "{what}: every refusal looks the same"
        );
    }

    async fn refuses_claims(&self, what: &str, claims: &Value) {
        self.refuses(what, &tokens::sign(&self.key, claims)).await;
    }
}

fn with(mut claims: Value, name: &str, value: Value) -> Value {
    claims[name] = value;
    claims
}

fn without(mut claims: Value, name: &str) -> Value {
    claims.as_object_mut().expect("object").remove(name);
    claims
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tokens_that_fail_a_check_are_all_refused_alike() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let s = Setup::start(Alg::Es256).await;
    assert!(
        s.accepts(&s.claims()).await,
        "the baseline token is accepted"
    );

    s.refuses_claims(
        "wrong audience",
        &with(s.claims(), "aud", json!("sts.amazonaws.com")),
    )
    .await;
    s.refuses_claims(
        "another tenant's audience",
        &with(
            s.claims(),
            "aud",
            json!("oxabl-cache:t_00000000000000000000000000"),
        ),
    )
    .await;
    s.refuses_claims(
        "audience without the prefix",
        &with(s.claims(), "aud", json!(s.env.tenant.id)),
    )
    .await;
    s.refuses_claims(
        "an audience list naming ours and another",
        &with(s.claims(), "aud", json!([s.env.tenant.audience(), "other"])),
    )
    .await;
    s.refuses_claims("no audience", &without(s.claims(), "aud"))
        .await;

    s.refuses_claims("expired", &with(s.claims(), "exp", json!(now() - 3600)))
        .await;
    s.refuses_claims(
        "expired beyond the skew",
        &with(s.claims(), "exp", json!(now() - 120)),
    )
    .await;
    assert!(
        s.accepts(&with(s.claims(), "exp", json!(now() - 30))).await,
        "expired within the 60 s skew is accepted"
    );
    s.refuses_claims(
        "not yet valid",
        &with(s.claims(), "nbf", json!(now() + 3600)),
    )
    .await;
    assert!(
        s.accepts(&with(s.claims(), "nbf", json!(now() + 30))).await,
        "nbf within the skew is accepted"
    );
    s.refuses_claims(
        "issued in the future",
        &with(s.claims(), "iat", json!(now() + 3600)),
    )
    .await;
    s.refuses_claims("no expiry", &without(s.claims(), "exp"))
        .await;
    s.refuses_claims("no issued-at", &without(s.claims(), "iat"))
        .await;
    s.refuses_claims("no subject", &without(s.claims(), "sub"))
        .await;

    s.refuses_claims(
        "an issuer the tenant did not register",
        &with(s.claims(), "iss", json!("https://evil.example")),
    )
    .await;
    s.refuses_claims(
        "the issuer URL with a trailing slash",
        &with(s.claims(), "iss", json!(format!("{}/", s.issuer))),
    )
    .await;

    let claims = s.claims();
    s.refuses("alg none", &tokens::unsigned("k1", &claims))
        .await;
    s.refuses(
        "an HMAC token keyed with the public key",
        &tokens::hmac_forged(&s.key, &claims),
    )
    .await;
    let stranger = SigningKey::generate(Alg::Es256, "k1");
    s.refuses(
        "a foreign key under the right kid",
        &tokens::sign(&stranger, &claims),
    )
    .await;
    let signed = tokens::sign(&s.key, &claims);
    let mut parts: Vec<&str> = signed.split('.').collect();
    let forged = tokens::sign(&s.key, &with(claims.clone(), "repository_id", json!("1")));
    let forged_payload = forged.split('.').nth(1).expect("payload");
    parts[1] = forged_payload;
    s.refuses("a payload swapped after signing", &parts.join("."))
        .await;
    for garbage in ["", "a.b.c", "....", &"A".repeat(20_000)] {
        s.refuses("a malformed token", garbage).await;
    }

    let unknown_tenant = s
        .env
        .exchange_for(
            "t_00000000000000000000000000",
            &tokens::sign(&s.key, &claims),
        )
        .await;
    assert_eq!(unknown_tenant.status, 401, "an unknown tenant");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_token_from_another_repository_with_the_same_environment_does_not_match() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let s = Setup::start(Alg::Rs256).await;
    let token = |repository: &str, id: &str, git_ref: &str, environment: &str| {
        tokens::sign(
            &s.key,
            &tokens::github(
                &s.issuer,
                &s.env.tenant.audience(),
                repository,
                id,
                git_ref,
                environment,
            ),
        )
    };
    assert_eq!(
        s.status(&token(
            REPOSITORY,
            REPOSITORY_ID,
            "refs/heads/main",
            "cache-writer"
        ))
        .await,
        200
    );
    s.refuses(
        "another repository, same environment name",
        &token(
            "evil/widgets",
            "111111111",
            "refs/heads/main",
            "cache-writer",
        ),
    )
    .await;
    s.refuses(
        "a renamed repository with another id",
        &token(REPOSITORY, "111111111", "refs/heads/main", "cache-writer"),
    )
    .await;
    s.refuses(
        "another ref",
        &token(
            REPOSITORY,
            REPOSITORY_ID,
            "refs/heads/feature",
            "cache-writer",
        ),
    )
    .await;
    s.refuses(
        "another environment",
        &token(REPOSITORY, REPOSITORY_ID, "refs/heads/main", "staging"),
    )
    .await;
    let mut claims = tokens::github(
        &s.issuer,
        &s.env.tenant.audience(),
        REPOSITORY,
        REPOSITORY_ID,
        "refs/heads/main",
        "cache-writer",
    );
    claims
        .as_object_mut()
        .expect("object")
        .remove("environment");
    s.refuses_claims("a missing claim", &claims).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unknown_kid_refetches_the_key_set_once() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let s = Setup::start(Alg::Es256).await;
    let fetches = || s.env.idp.seen_paths("/github");
    let registered = fetches();
    assert!(s.accepts(&s.claims()).await);
    let after_first = fetches();
    assert_eq!(
        after_first,
        registered + 2,
        "discovery and the key set are fetched once and cached"
    );
    assert!(s.accepts(&s.claims()).await);
    assert_eq!(
        fetches(),
        after_first,
        "a known kid is served from the cache"
    );

    let rotated = key(Alg::Es256, "k2");
    s.env.idp.publish("/github", vec![rotated.clone()]);
    tokio::time::sleep(Duration::from_millis(1300)).await;
    let token = tokens::sign(&rotated, &s.claims());
    assert_eq!(
        s.status(&token).await,
        200,
        "the rotated key is found by one refetch"
    );
    assert_eq!(fetches(), after_first + 2);

    let ghost = tokens::sign_as(&rotated, "ghost", &s.claims());
    tokio::time::sleep(Duration::from_millis(1300)).await;
    s.refuses("a kid no key set has", &ghost).await;
    assert_eq!(fetches(), after_first + 4, "refetched once, then refused");
    s.refuses("the same unknown kid again at once", &ghost)
        .await;
    assert_eq!(
        fetches(),
        after_first + 4,
        "unknown kids cannot drive a fetch storm"
    );
}

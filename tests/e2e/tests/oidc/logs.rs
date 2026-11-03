use crate::support::{ADDRESSES, Env, REPOSITORY, REPOSITORY_ID, key, token_of};
use oxabl_cache_e2e::idp::keys::{Alg, SigningKey};
use oxabl_cache_e2e::idp::tokens;
use oxabl_cache_e2e::netns;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logs_hold_no_jwt_claim_or_session() {
    if !netns::enter(&ADDRESSES) {
        return;
    }
    let env = Env::start().await;
    let signing = key(Alg::Rs256, "k1");
    let issuer = env.github_writer("/github", signing.clone()).await;
    let claims = tokens::github(
        &issuer,
        &env.tenant.audience(),
        REPOSITORY,
        REPOSITORY_ID,
        "refs/heads/main",
        "cache-writer",
    );
    let good = tokens::sign(&signing, &claims);
    let mut jwts = vec![good.clone()];
    jwts.push(tokens::sign(
        &SigningKey::generate(Alg::Rs256, "k1"),
        &claims,
    ));
    jwts.push(tokens::unsigned("k1", &claims));
    jwts.push(tokens::hmac_forged(&signing, &claims));
    jwts.push(tokens::sign(
        &signing,
        &tokens::github(
            &issuer,
            "wrong-audience",
            REPOSITORY,
            REPOSITORY_ID,
            "refs/heads/main",
            "cache-writer",
        ),
    ));
    let session = token_of(&env.exchange(&good).await);
    for jwt in jwts.iter().skip(1) {
        assert_eq!(env.exchange(jwt).await.status, 401);
    }
    let rejected = env
        .register("https://private.test/secret-issuer-path", "github", &[])
        .await;
    assert_eq!(rejected.status, 400);
    let denied = env.exchange("not-a-jwt.at.all").await;
    assert_eq!(denied.status, 401);

    let log = env.server.log();
    assert!(
        log.contains("\"route\":\"/v1/auth/oidc\""),
        "the exchanges were logged at all:\n{log}"
    );
    for jwt in &jwts {
        for part in jwt.split('.').filter(|part| part.len() > 8) {
            assert!(!log.contains(part), "a JWT segment is in the log: {part}");
        }
    }
    for secret in [
        session.as_str(),
        session.rsplit('_').next().expect("secret"),
        env.tenant.admin.as_str(),
        REPOSITORY_ID,
        "refs/heads/main",
        "cache-writer",
        REPOSITORY,
        "not-a-jwt",
        "secret-issuer-path",
    ] {
        assert!(!log.contains(secret), "{secret} is in the log");
    }
}

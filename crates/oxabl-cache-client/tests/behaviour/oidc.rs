//! OIDC: an ID token leaves the process only if its audience is exactly the tenant's.

use oxabl_cache_client::{Cache, Credentials, IdTokenError, Mode, OpenError, SecretString};
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::driver;
use crate::support::issuer::{Issuer, jwt};
use crate::support::world::World;

fn audience(world: &World) -> String {
    format!("oxabl-cache:{}", world.tenant)
}

fn github_env(world: &World, issuer: &Issuer, state: &TestDir) -> Vec<(String, String)> {
    vec![
        ("OXABL_CACHE_IDENTITY".into(), world.ci.identity.clone()),
        ("XDG_STATE_HOME".into(), state.path().display().to_string()),
        ("ACTIONS_ID_TOKEN_REQUEST_URL".into(), issuer.url()),
        (
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN".into(),
            "runner-secret".into(),
        ),
    ]
}

fn exchanges(world: &World) -> Vec<String> {
    world
        .proxy
        .seen()
        .into_iter()
        .filter(|s| s.path == "/v1/auth/oidc")
        .map(|s| String::from_utf8_lossy(&s.body).into_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_github_job_asks_for_the_tenants_audience_and_exchanges_the_token() {
    let world = World::start().await;
    world.proxy.oidc_session(Some(world.ci.token.clone()));
    let issuer = Issuer::start(None).await;
    let state = TestDir::new("state").expect("directory");
    let answer = driver::open(world.repo(), github_env(&world, &issuer, &state)).await;
    assert_eq!(answer["open"], "ok", "{answer}");

    let asked = issuer.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].audience.as_deref(),
        Some(audience(&world).as_str())
    );
    assert_eq!(
        asked[0].authorization.as_deref(),
        Some("Bearer runner-secret")
    );
    assert!(
        asked[0].query.contains("api-version=2"),
        "the job's own query is kept"
    );
    let sent = exchanges(&world);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains(&world.tenant) && sent[0].contains(&jwt(&audience(&world))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_token_for_another_audience_is_never_sent_whatever_the_source() {
    let world = World::start().await;
    world.proxy.oidc_session(Some(world.ci.token.clone()));
    let state = TestDir::new("state").expect("directory");

    let hostile = Issuer::start(Some("sts.amazonaws.com".into())).await;
    let answer = driver::open(world.repo(), github_env(&world, &hostile, &state)).await;
    let error = answer["open_error"]
        .as_str()
        .unwrap_or_else(|| panic!("{answer}"));
    assert!(error.contains("AudienceMismatch"), "{error}");

    let wrong = jwt("sts.amazonaws.com");
    let gitlab = vec![
        ("OXABL_CACHE_IDENTITY".to_owned(), world.ci.identity.clone()),
        (
            "XDG_STATE_HOME".to_owned(),
            state.path().display().to_string(),
        ),
        ("OXABL_CACHE_ID_TOKEN".to_owned(), wrong.clone()),
    ];
    let answer = driver::open(world.repo(), gitlab).await;
    assert!(
        answer["open_error"]
            .as_str()
            .unwrap_or_default()
            .contains("AudienceMismatch"),
        "{answer}"
    );

    let explicit = Cache::open(
        world
            .config(&world.ci, Mode::Ci, state.path())
            .with_credentials(Credentials::OidcIdToken(SecretString::from(wrong.clone()))),
    )
    .await
    .err()
    .expect("refused");
    assert_eq!(explicit, OpenError::IdToken(IdTokenError::AudienceMismatch));

    assert!(
        world
            .proxy
            .seen()
            .iter()
            .all(|s| !String::from_utf8_lossy(&s.body).contains(&wrong)
                && s.authorization
                    .as_deref()
                    .is_none_or(|a| !a.contains(&wrong))),
        "the proxy never saw the token"
    );
    assert!(exchanges(&world).is_empty(), "no exchange was attempted");

    let good = vec![
        ("OXABL_CACHE_IDENTITY".to_owned(), world.ci.identity.clone()),
        (
            "XDG_STATE_HOME".to_owned(),
            state.path().display().to_string(),
        ),
        ("OXABL_CACHE_ID_TOKEN".to_owned(), jwt(&audience(&world))),
    ];
    assert_eq!(driver::open(world.repo(), good).await["open"], "ok");
    assert_eq!(exchanges(&world).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_audience_never_comes_from_the_server_and_a_401_renews_the_session() {
    let world = World::start().await;
    world.proxy.oidc_session(Some(world.ci.token.clone()));
    let state = TestDir::new("state").expect("directory");
    let token = jwt(&audience(&world));
    let cache = Cache::open(
        world
            .config(&world.ci, Mode::Ci, state.path())
            .with_credentials(Credentials::OidcIdToken(SecretString::from(token.clone()))),
    )
    .await
    .expect("open");
    assert_eq!(exchanges(&world).len(), 1);

    world.proxy.unauthorized_once();
    let probed = cache.probe(&[crate::support::fixture::key(1)]).await;
    assert!(
        probed.is_ok(),
        "the 401 was answered by a new session: {probed:?}"
    );
    let sent = exchanges(&world);
    assert_eq!(sent.len(), 2, "the session was renewed once");
    assert!(
        sent.iter()
            .all(|body| body.contains(&token) && !body.contains("someone-else")),
        "the audience of a renewal is still the tenant's, whatever the 401 said"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_token_in_the_environment_wins_and_the_issuer_is_not_asked() {
    let world = World::start().await;
    let issuer = Issuer::start(None).await;
    let state = TestDir::new("state").expect("directory");
    let mut env = github_env(&world, &issuer, &state);
    env.push(("OXABL_CACHE_TOKEN".into(), world.ci.token.clone()));
    let answer = driver::open(world.repo(), env).await;
    assert_eq!(answer["open"], "ok", "{answer}");
    assert!(issuer.asked().is_empty());
    assert!(exchanges(&world).is_empty());
}

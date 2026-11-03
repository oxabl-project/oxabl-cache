//! Identities and stored sessions: where they come from and what refuses them.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{SystemTime, UNIX_EPOCH};

use oxabl_cache_client::{Cache, IdentitySource, Mode, OpenError, SecretString};
use oxabl_cache_crypto::{Identity, identity_file};
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::driver;
use crate::support::world::{World, identity_of};

fn protected(world: &World, dir: &TestDir, passphrase: &str) -> std::path::PathBuf {
    let path = dir.path().join("identity");
    let bytes = identity_file::protect_with_work_factor(
        &identity_of(&world.ci),
        &SecretString::from(passphrase.to_owned()),
        4,
    )
    .expect("protect");
    fs::write(&path, bytes).expect("write");
    path
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_protected_identity_file_asks_the_callback_and_falls_back_to_the_environment() {
    let world = World::start().await;
    let dir = TestDir::new("identity").expect("directory");
    let path = protected(&world, &dir, "correct horse");
    let state = TestDir::new("state").expect("directory");
    let config = |passphrase: Option<&'static str>| {
        let config = world
            .config(&world.ci, Mode::Ci, state.path())
            .with_identity(IdentitySource::File(path.clone()));
        match passphrase {
            Some(p) => config.with_passphrase(move || Some(SecretString::from(p.to_owned()))),
            None => config.with_passphrase(|| None),
        }
    };
    Cache::open(config(Some("correct horse")))
        .await
        .expect("the right passphrase");
    for refused in [Some("wrong"), None] {
        let error = Cache::open(config(refused)).await.err().expect("refused");
        assert!(matches!(error, OpenError::Identity(_)), "{error:?}");
    }

    let env = vec![
        ("OXABL_CACHE_TOKEN".to_owned(), world.ci.token.clone()),
        (
            "OXABL_CACHE_IDENTITY_FILE".to_owned(),
            path.display().to_string(),
        ),
        (
            "OXABL_CACHE_IDENTITY_PASSPHRASE".to_owned(),
            "correct horse".to_owned(),
        ),
        (
            "XDG_STATE_HOME".to_owned(),
            state.path().display().to_string(),
        ),
    ];
    assert_eq!(driver::open(world.repo(), env.clone()).await["open"], "ok");
    let mut wrong = env;
    wrong[2].1 = "wrong".into();
    let answer = driver::open(world.repo(), wrong).await;
    assert!(
        answer["open_error"]
            .as_str()
            .unwrap_or_default()
            .contains("Identity"),
        "{answer}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_identity_that_is_not_in_the_roster_is_refused() {
    let world = World::start().await;
    let state = TestDir::new("state").expect("directory");
    let stranger = Identity::generate().expect("identity");
    let error = Cache::open(
        world
            .config(&world.ci, Mode::Ci, state.path())
            .with_identity(IdentitySource::Value(
                stranger.to_secret_string().expect("string"),
            )),
    )
    .await
    .err()
    .expect("refused");
    assert_eq!(error, OpenError::IdentityNotInRoster);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stored_session_is_used_when_nothing_else_is_given() {
    let world = World::start().await;
    let state = TestDir::new("state").expect("directory");
    let dir = state.path().join("oxabl-cache/session");
    fs::create_dir_all(&dir).expect("directory");
    let file = dir.join(&world.tenant);
    let env = vec![
        ("OXABL_CACHE_IDENTITY".to_owned(), world.ci.identity.clone()),
        (
            "XDG_STATE_HOME".to_owned(),
            state.path().display().to_string(),
        ),
    ];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let write = |expires: u64, mode: u32| {
        fs::write(
            &file,
            serde_json::json!({"token": world.ci.token, "expires_at": expires}).to_string(),
        )
        .expect("write");
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).expect("mode");
    };

    let none = driver::open(world.repo(), env.clone()).await;
    assert_eq!(none["open_error"], "NoCredentials", "{none}");
    write(now - 10, 0o600);
    assert_eq!(
        driver::open(world.repo(), env.clone()).await["open_error"],
        "NoCredentials"
    );
    write(now + 3_600, 0o644);
    let loose = driver::open(world.repo(), env.clone()).await;
    assert!(
        loose["open_error"]
            .as_str()
            .unwrap_or_default()
            .starts_with("State"),
        "{loose}"
    );
    write(now + 3_600, 0o600);
    assert_eq!(driver::open(world.repo(), env).await["open"], "ok");
}

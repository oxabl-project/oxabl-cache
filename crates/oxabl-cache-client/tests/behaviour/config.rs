//! Configuration: the committed trust root and what the environment may and may not change.

use std::fs;
use std::os::unix::fs::symlink;

use oxabl_cache_client::{Config, ConfigError};
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::driver;
use crate::support::world::{World, owner_key_text};

const TENANT: &str = "t_2h8k4m6p9q1r3s5t7v0w2x4y6z";
const CACHE: &str = "c_6v2k9q3m8x1r4t7w0y5z2b8n4d";

fn repo_with(text: &str) -> TestDir {
    let repo = TestDir::new("config").expect("directory");
    fs::create_dir_all(repo.path().join(".oxabl")).expect("directory");
    fs::write(repo.path().join(".oxabl/cache.toml"), text).expect("file");
    repo
}

fn valid(extra: &str) -> String {
    format!(
        "server = \"https://cache.example.com\"\ntenant = \"{TENANT}\"\ncache = \"{CACHE}\"\nowner_keys = [\"{}\"]\nmin_roster_version = 12\n{extra}",
        owner_key_text(&[7; 32])
    )
}

#[test]
fn the_five_keys_are_read_and_nothing_else_is_accepted() {
    let repo = repo_with(&valid(""));
    Config::from_repo(repo.path()).expect("a valid file");

    let repo = repo_with(&valid("branch = \"main\"\n"));
    assert_eq!(
        Config::from_repo(repo.path()).unwrap_err(),
        ConfigError::UnknownKey("branch".into())
    );

    let missing = valid("").replace("min_roster_version = 12\n", "");
    let repo = repo_with(&missing);
    assert!(matches!(
        Config::from_repo(repo.path()),
        Err(ConfigError::Parse(_))
    ));
}

#[test]
fn owner_keys_have_exactly_one_spelling() {
    let key = owner_key_text(&[7; 32]);
    let bare = key.trim_start_matches("ed25519:");
    let spellings = [
        format!("{key}="),
        format!("{key}x"),
        format!("ed25519: {bare}"),
        format!("ed25519:{bare} "),
        bare.to_owned(),
        "ed25519:".to_owned(),
        // the last character carries two unused bits: flipping one is a different spelling
        format!("{}B", &key[..key.len() - 1]),
    ];
    for spelling in spellings {
        let text = valid("").replace(&key, &spelling);
        let repo = repo_with(&text);
        assert!(
            matches!(Config::from_repo(repo.path()), Err(ConfigError::Invalid(_))),
            "{spelling:?} must be refused"
        );
    }
}

#[test]
fn only_the_standard_alphabet_is_a_spelling() {
    let key = owner_key_text(&[0xfb; 32]);
    assert!(key.contains('+') || key.contains('/'));
    let accepted = valid("").replace(&owner_key_text(&[7; 32]), &key);
    Config::from_repo(repo_with(&accepted).path()).expect("the standard alphabet");
    let url_safe = key.replace('+', "-").replace('/', "_");
    let refused = valid("").replace(&owner_key_text(&[7; 32]), &url_safe);
    assert!(matches!(
        Config::from_repo(repo_with(&refused).path()),
        Err(ConfigError::Invalid(_))
    ));
}

#[test]
fn the_file_must_be_a_regular_file_inside_the_repository() {
    let repo = TestDir::new("config").expect("directory");
    let outside = TestDir::new("outside").expect("directory");
    fs::write(outside.path().join("cache.toml"), valid("")).expect("file");
    fs::create_dir_all(repo.path().join(".oxabl")).expect("directory");
    symlink(
        outside.path().join("cache.toml"),
        repo.path().join(".oxabl/cache.toml"),
    )
    .expect("link");
    assert!(matches!(
        Config::from_repo(repo.path()),
        Err(ConfigError::NotRegularFile(_))
    ));

    let inside = TestDir::new("config").expect("directory");
    fs::create_dir_all(inside.path().join(".oxabl")).expect("directory");
    fs::write(inside.path().join("real.toml"), valid("")).expect("file");
    symlink(
        inside.path().join("real.toml"),
        inside.path().join(".oxabl/cache.toml"),
    )
    .expect("link");
    assert!(
        matches!(
            Config::from_repo(inside.path()),
            Err(ConfigError::NotRegularFile(_))
        ),
        "a link is refused even when it stays inside the repository"
    );

    let linked = TestDir::new("config").expect("directory");
    symlink(outside.path(), linked.path().join(".oxabl")).expect("link");
    assert!(matches!(
        Config::from_repo(linked.path()),
        Err(ConfigError::NotRegularFile(_))
    ));

    let directory = TestDir::new("config").expect("directory");
    fs::create_dir_all(directory.path().join(".oxabl/cache.toml")).expect("directory");
    assert!(matches!(
        Config::from_repo(directory.path()),
        Err(ConfigError::NotRegularFile(_))
    ));

    let none = TestDir::new("config").expect("directory");
    assert!(matches!(
        Config::from_repo(none.path()),
        Err(ConfigError::MissingFile(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_environment_variable_cannot_override_a_trust_field() {
    let world = World::start().await;
    let state = TestDir::new("state").expect("directory");
    let control = driver::open(world.repo(), world.environment(&world.ci, state.path())).await;
    assert_eq!(control["open"], "ok", "{control}");

    for variable in [
        "OXABL_CACHE_TENANT",
        "OXABL_CACHE_CACHE",
        "OXABL_CACHE_OWNER_KEYS",
        "OXABL_CACHE_MIN_ROSTER_VERSION",
    ] {
        let mut env = world.environment(&world.ci, state.path());
        env.push((variable.to_owned(), "anything".to_owned()));
        let answer = driver::open(world.repo(), env).await;
        let error = answer["config_error"]
            .as_str()
            .unwrap_or_else(|| panic!("{variable}: {answer}"));
        assert!(
            error.contains("TrustOverride") && error.contains(variable),
            "{variable}: {error}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_server_may_be_overridden_and_the_mode_defaults_from_ci() {
    let world = World::start().await;
    let state = TestDir::new("state").expect("directory");
    let elsewhere = TestDir::new("elsewhere").expect("directory");
    let dead = "http://127.0.0.1:1".to_owned();
    fs::create_dir_all(elsewhere.path().join(".oxabl")).expect("directory");
    let toml = fs::read_to_string(world.repo().join(".oxabl/cache.toml")).expect("file");
    fs::write(
        elsewhere.path().join(".oxabl/cache.toml"),
        toml.replace(&world.proxy.url(), &dead),
    )
    .expect("file");

    let env = world.environment(&world.ci, state.path());
    let unreachable = driver::open(elsewhere.path(), env.clone()).await;
    assert_eq!(unreachable["open_error"], "Unreachable", "{unreachable}");

    let mut redirected = env.clone();
    redirected.push(("OXABL_CACHE_SERVER".into(), world.proxy.url()));
    let opened = driver::open(elsewhere.path(), redirected.clone()).await;
    assert_eq!(
        opened["open"], "ok",
        "OXABL_CACHE_SERVER is honoured: {opened}"
    );
    assert_eq!(opened["mode"], "Developer", "no CI variable: developer");

    redirected.push(("CI".into(), "true".into()));
    assert_eq!(
        driver::open(elsewhere.path(), redirected.clone()).await["mode"],
        "ReadOnly"
    );

    redirected.push(("OXABL_CACHE_MODE".into(), "ci".into()));
    assert_eq!(
        driver::open(elsewhere.path(), redirected).await["mode"],
        "Ci"
    );
}

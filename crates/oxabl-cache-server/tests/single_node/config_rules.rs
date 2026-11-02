//! The configuration is strict, secrets live in files, and plaintext is explicit.

use crate::support::cli::{run, run_many};

const BASE: &str = "[server]
listen = \"127.0.0.1:0\"
[metadata]
url = \"sqlite://{dir}/meta.db\"
[store]
kind = \"fs\"
root = \"{dir}/objects\"
[keys]
url_key_file = \"{dir}/url.key\"
";

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn unknown_keys_and_inline_secrets_are_errors() {
    let unknown = run(
        "config-unknown",
        &format!("{BASE}[gc]\nintervall = \"1s\"\n"),
        &["check-store"],
        &[],
    );
    assert!(!unknown.status.success());
    assert!(
        stderr(&unknown).contains("intervall"),
        "{}",
        stderr(&unknown)
    );
    let section = run(
        "config-section",
        &format!("{BASE}[extra]\nx = 1\n"),
        &["check-store"],
        &[],
    );
    assert!(!section.status.success());
    let inline = run(
        "config-inline-secret",
        &BASE.replace("[keys]", "[keys]\nurl_key = \"00\"\n"),
        &["check-store"],
        &[],
    );
    assert!(
        !inline.status.success(),
        "a secret written into the file is refused"
    );
    let from_env = run(
        "config-env-secret",
        BASE,
        &["check-store"],
        &[("OXABL_CACHED_KEYS_URL_KEY", "00")],
    );
    assert!(
        !from_env.status.success(),
        "a secret value is not an environment override either"
    );
}

#[test]
fn plaintext_beyond_loopback_needs_an_explicit_upstream_terminator() {
    let open = BASE.replace(
        "listen = \"127.0.0.1:0\"",
        "listen = \"0.0.0.0:0\"\npublic_url = \"https://cache.example\"",
    );
    let refused = run("config-plain", &open, &["check-store"], &[]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("loopback"),
        "{}",
        stderr(&refused)
    );
    let tls_missing_key = run(
        "config-tls-half",
        &format!("{open}[tls]\ncert_file = \"/nope.crt\"\n"),
        &["check-store"],
        &[],
    );
    assert!(!tls_missing_key.status.success());

    let behind_proxy = run(
        "config-upstream",
        &format!("{open}[tls]\nterminated_upstream = true\n"),
        &["init"],
        &[],
    );
    assert!(behind_proxy.status.success(), "{}", stderr(&behind_proxy));
    let by_env = run(
        "config-env-override",
        &open,
        &["init"],
        &[("OXABL_CACHED_TLS_TERMINATED_UPSTREAM", "true")],
    );
    assert!(
        by_env.status.success(),
        "environment overrides apply: {}",
        stderr(&by_env)
    );
}

#[test]
fn init_mints_the_first_operator_token_once_and_unbuilt_subcommands_say_so() {
    let first = run("init-once", BASE, &["init"], &[]);
    assert!(first.status.success(), "{}", stderr(&first));
    let token = String::from_utf8_lossy(&first.stdout).trim().to_owned();
    assert!(token.starts_with("oxo_"), "{token}");

    for command in ["migrate", "backup"] {
        let output = run("unbuilt", BASE, &[command], &[]);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            stderr(&output).contains("not implemented"),
            "{}",
            stderr(&output)
        );
    }
    let postgres = run(
        "config-postgres",
        &BASE.replace("sqlite://{dir}/meta.db", "postgres://u@h/db"),
        &["check-store"],
        &[],
    );
    assert!(!postgres.status.success());
    assert!(stderr(&postgres).contains("not available"));
}

#[test]
fn a_wildcard_listener_needs_a_public_url_and_check_store_reports_the_capabilities() {
    let wildcard = BASE.replace("127.0.0.1:0", "0.0.0.0:0");
    let refused = run(
        "config-public-url",
        &format!("{wildcard}[tls]\nterminated_upstream = true\n"),
        &["init"],
        &[],
    );
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("public_url"),
        "{}",
        stderr(&refused)
    );

    let outputs = run_many(
        "config-check-store",
        BASE,
        &[&["init"], &["check-store"]],
        &[],
    );
    assert!(outputs[1].status.success(), "{}", stderr(&outputs[1]));
    let capabilities: serde_json::Value =
        serde_json::from_slice(&outputs[1].stdout).expect("check-store prints JSON");
    assert_eq!(capabilities["kind"], "fs");
    assert_eq!(capabilities["if_none_match"], false);
}

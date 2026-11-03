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
fn init_mints_the_first_operator_token_once_and_the_copy_commands_need_their_options() {
    let first = run("init-once", BASE, &["init"], &[]);
    assert!(first.status.success(), "{}", stderr(&first));
    let token = String::from_utf8_lossy(&first.stdout).trim().to_owned();
    assert!(token.starts_with("oxo_"), "{token}");

    for (command, option) in [("migrate", "--from"), ("backup", "--to")] {
        let output = run("copy-options", BASE, &[command], &[]);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains(option), "{}", stderr(&output));
    }
}

#[test]
fn postgres_metadata_is_configured_by_url_and_password_file_and_fails_closed() {
    let postgres = |url: &str| BASE.replace("sqlite://{dir}/meta.db", url);
    let embedded = run(
        "config-postgres-password",
        &postgres("postgres://u:secret@h/db"),
        &["check-store"],
        &[],
    );
    assert!(!embedded.status.success());
    assert!(
        stderr(&embedded).contains("password_file"),
        "{}",
        stderr(&embedded)
    );

    let sqlite_with_password = run(
        "config-sqlite-password",
        &BASE.replace("[metadata]\n", "[metadata]\npassword_file = \"{dir}/pw\"\n"),
        &["check-store"],
        &[],
    );
    assert!(!sqlite_with_password.status.success());
    assert!(
        stderr(&sqlite_with_password).contains("postgres only"),
        "{}",
        stderr(&sqlite_with_password)
    );

    let unreachable = run(
        "config-postgres-unreachable",
        &postgres("postgres://u@127.0.0.1:1/db?sslmode=disable"),
        &["check-store"],
        &[],
    );
    assert!(!unreachable.status.success());
    assert!(
        stderr(&unreachable).contains("cannot open the metadata database"),
        "{}",
        stderr(&unreachable)
    );

    let no_password_file = run(
        "config-postgres-password-file",
        &BASE.replace(
            "[metadata]\nurl = \"sqlite://{dir}/meta.db\"",
            "[metadata]\nurl = \"postgres://u@127.0.0.1:1/db?sslmode=disable\"\npassword_file = \"{dir}/missing\"",
        ),
        &["check-store"],
        &[],
    );
    assert!(!no_password_file.status.success());
    assert!(
        stderr(&no_password_file).contains("password file"),
        "{}",
        stderr(&no_password_file)
    );
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

#[test]
fn the_gc_batch_must_be_a_positive_number() {
    let zero = run(
        "config-gc-batch",
        &format!("{BASE}[gc]\nbatch = 0\n"),
        &["check-store"],
        &[],
    );
    assert!(!zero.status.success());
    assert!(stderr(&zero).contains("gc.batch"), "{}", stderr(&zero));
    let fine = run(
        "config-gc-batch-ok",
        &format!("{BASE}[gc]\nbatch = 25\n"),
        &["init"],
        &[],
    );
    assert!(fine.status.success(), "{}", stderr(&fine));
}

#[test]
fn the_gc_grace_must_exceed_the_url_lifetime() {
    let with = |name: &str, lifetime: &str, grace: &str| {
        let server = format!("listen = \"127.0.0.1:0\"\nurl_lifetime = \"{lifetime}\"\n");
        let text = BASE.replacen("listen = \"127.0.0.1:0\"\n", &server, 1);
        run(
            name,
            &format!("{text}[gc]\ngrace = \"{grace}\"\n"),
            &["init"],
            &[],
        )
    };
    for (name, lifetime, grace) in [
        ("grace-shorter", "15m", "10m"),
        ("grace-equal", "5s", "5s"),
        ("grace-just-short", "5s", "5500ms"),
    ] {
        let refused = with(name, lifetime, grace);
        assert!(!refused.status.success(), "{lifetime} against {grace}");
        let message = stderr(&refused);
        assert!(
            message.contains("gc.grace") && message.contains("server.url_lifetime"),
            "{message}"
        );
    }
    let fine = with("grace-just-enough", "5s", "6s");
    assert!(fine.status.success(), "{}", stderr(&fine));
    let defaults = run("grace-defaults", BASE, &["init"], &[]);
    assert!(defaults.status.success(), "{}", stderr(&defaults));
}

#[test]
fn backup_is_refused_on_postgres_metadata_with_a_pointer_to_base_backups() {
    let postgres = BASE.replace(
        "url = \"sqlite://{dir}/meta.db\"",
        "url = \"postgres://u@127.0.0.1:1/db?sslmode=disable\"",
    );
    let refused = run(
        "backup-postgres",
        &postgres,
        &["backup", "--to", "{dir}/copy.db"],
        &[],
    );
    assert!(!refused.status.success());
    let message = stderr(&refused);
    assert!(
        message.contains("SQLite") && message.contains("WAL"),
        "{message}"
    );
}

#[test]
fn the_postgres_timeouts_are_validated_and_apply_to_postgres_only() {
    let postgres = |keys: &str| {
        BASE.replace(
            "url = \"sqlite://{dir}/meta.db\"",
            &format!("url = \"postgres://u@127.0.0.1:1/db?sslmode=disable\"\n{keys}"),
        )
    };
    for (keys, mentioned) in [
        ("statement_timeout = \"0s\"", "statement_timeout"),
        ("statement_timeout = \"2h\"", "statement_timeout"),
        ("statement_timeout = \"soon\"", "soon"),
        (
            "idle_in_transaction_timeout = \"0s\"",
            "idle_in_transaction_timeout",
        ),
        (
            "idle_in_transaction_timeout = \"1d\"",
            "idle_in_transaction_timeout",
        ),
        ("max_connections = 0", "max_connections"),
    ] {
        let refused = run("config-pg-timeouts", &postgres(keys), &["check-store"], &[]);
        assert!(!refused.status.success(), "{keys}");
        assert!(
            stderr(&refused).contains(mentioned),
            "{keys}: {}",
            stderr(&refused)
        );
    }
    let accepted = run(
        "config-pg-timeouts-ok",
        &postgres("statement_timeout = \"30s\"\nidle_in_transaction_timeout = \"5s\""),
        &["check-store"],
        &[],
    );
    assert!(
        stderr(&accepted).contains("cannot open the metadata database"),
        "valid timeouts pass validation and reach the connection: {}",
        stderr(&accepted)
    );

    let sqlite = run(
        "config-sqlite-timeouts",
        &BASE.replace("[metadata]\n", "[metadata]\nstatement_timeout = \"30s\"\n"),
        &["check-store"],
        &[],
    );
    assert!(!sqlite.status.success());
    assert!(
        stderr(&sqlite).contains("postgres only"),
        "{}",
        stderr(&sqlite)
    );
}

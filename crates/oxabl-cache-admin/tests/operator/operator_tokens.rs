//! Minting a second operator token, revoking the first, and the guard on the last.

use crate::call;
use crate::support::admin::{run, run_with};
use crate::support::server::Server;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn token_file(server: &Server, name: &str, secret: &str) -> PathBuf {
    let path = server.dir.join(name);
    fs::write(&path, secret).expect("token file");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("mode");
    path
}

#[test]
fn a_second_operator_token_is_minted_then_the_first_is_revoked_and_refused_at_once() {
    let server = Server::start("op-operator-tokens", false);
    let first = server.operator_token();
    let listed = run(&server, &["--json", "operator-token", "list"]).json();
    let tokens = listed["operator_tokens"].as_array().expect("tokens");
    assert_eq!(tokens.len(), 1);
    let first_id = tokens[0]["id"].as_str().expect("id").to_owned();
    assert_eq!(tokens[0]["revoked_at"], serde_json::Value::Null);
    assert!(
        !listed.to_string().contains(&first),
        "a listing never shows the secret"
    );

    let created = run(
        &server,
        &[
            "--json",
            "operator-token",
            "create",
            "--name",
            "ops-2",
            "--expires",
            "30d",
        ],
    );
    assert_eq!(created.code(), 0, "{}", created.stderr());
    let second = created.json()["token"].as_str().expect("token").to_owned();
    assert!(second.starts_with("oxo_"), "{second}");
    let second_file = token_file(&server, "second.token", &second);
    let by_second = run_with(&server, &second_file, None, &["health"]);
    assert_eq!(by_second.code(), 0, "{}", by_second.stderr());

    let revoked = run_with(
        &server,
        &second_file,
        None,
        &["--json", "operator-token", "revoke", &first_id],
    );
    assert_eq!(revoked.code(), 0, "{}", revoked.stderr());
    let by_first = run(&server, &["health"]);
    assert_eq!(by_first.code(), 1, "the revoked token is refused at once");
    assert!(
        by_first.stderr().contains("unauthenticated"),
        "{}",
        by_first.stderr()
    );
    let direct = call(
        &server.admin_url(),
        &first,
        "GET",
        "/admin/v1/tenants",
        None,
    );
    assert_eq!(direct.status(), 401);
    let again = run_with(
        &server,
        &second_file,
        None,
        &["operator-token", "revoke", &first_id],
    );
    assert_eq!(again.code(), 1, "a token cannot be revoked twice");

    let after = run_with(
        &server,
        &second_file,
        None,
        &["--json", "operator-token", "list"],
    )
    .json();
    let rows = after["operator_tokens"].as_array().expect("tokens");
    assert_eq!(rows.len(), 2);
    let row = |id: &str| rows.iter().find(|row| row["id"] == id).expect("row");
    assert!(
        row(&first_id)["revoked_at"].is_i64(),
        "the revoked token shows when"
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row["revoked_at"].is_null())
            .count(),
        1,
        "the second token is still valid"
    );
    assert_eq!(by_second.code(), 0);
}

#[test]
fn the_last_valid_operator_token_cannot_be_revoked_and_bad_input_is_refused() {
    let server = Server::start("op-operator-last", false);
    let listed = run(&server, &["--json", "operator-token", "list"]).json();
    let only = listed["operator_tokens"][0]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let refused = run(&server, &["operator-token", "revoke", &only]);
    assert_eq!(refused.code(), 1);
    assert!(
        refused.stderr().contains("last_operator_token"),
        "{}",
        refused.stderr()
    );
    assert_eq!(run(&server, &["health"]).code(), 0, "the token still works");

    assert_eq!(
        run(&server, &["operator-token", "revoke", "nobody"]).code(),
        1
    );
    assert_eq!(
        run(&server, &["operator-token", "revoke", "../x"]).code(),
        2
    );
    assert_eq!(
        run(&server, &["operator-token", "create", "--expires", "soon"]).code(),
        2
    );
    let human = run(&server, &["operator-token", "create"]);
    assert!(
        human
            .stdout()
            .lines()
            .any(|line| line.starts_with("token") && line.len() > 20),
        "{}",
        human.stdout()
    );

    let spare = run(&server, &["--json", "operator-token", "create"]).json();
    let spare_id = spare["id"].as_str().expect("id");
    assert_eq!(
        run(&server, &["operator-token", "revoke", spare_id]).code(),
        0,
        "with another valid token left, a token can go"
    );
}

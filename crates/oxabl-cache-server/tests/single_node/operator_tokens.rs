//! Operator tokens over the admin listener: a second token, revocation, the guard on the last one.

use crate::support::fixture::World;
use crate::support::server::{Options, call};
use serde_json::json;

#[test]
fn a_revoked_operator_token_is_401_at_once_and_the_last_valid_one_is_kept() {
    let world = World::start("operator-tokens", Options::default());
    let server = &world.server;
    let first = server.operator.clone();
    let listed = server
        .admin_call("GET", "/admin/v1/operator-tokens", None)
        .json();
    let first_id = listed["operator_tokens"][0]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let created = server.admin_call(
        "POST",
        "/admin/v1/operator-tokens",
        Some(&json!({"name": "second", "expires_in_days": 30})),
    );
    assert_eq!(
        created.status,
        201,
        "{}",
        String::from_utf8_lossy(&created.body)
    );
    let second = created.json()["token"].as_str().expect("token").to_owned();
    let second_id = created.json()["id"].as_str().expect("id").to_owned();
    let by = |token: &str, method: &str, path: &str| call(&server.admin, token, method, path, None);
    assert_eq!(by(&second, "GET", "/admin/v1/health").status, 200);

    let revoked = by(
        &second,
        "DELETE",
        &format!("/admin/v1/operator-tokens/{first_id}"),
    );
    assert_eq!(revoked.status, 204);
    assert_eq!(
        by(&first, "GET", "/admin/v1/health").status,
        401,
        "the revoked token is refused on its next call"
    );
    assert_eq!(
        by(
            &second,
            "DELETE",
            &format!("/admin/v1/operator-tokens/{first_id}")
        )
        .status,
        404,
        "revoking twice finds nothing"
    );
    let last = by(
        &second,
        "DELETE",
        &format!("/admin/v1/operator-tokens/{second_id}"),
    );
    assert_eq!(last.status, 409);
    assert_eq!(last.json()["error"]["reason"], "last_operator_token");
    assert_eq!(by(&second, "GET", "/admin/v1/health").status, 200);

    let tenant_token = world.admin.token.clone();
    assert_eq!(
        by(&tenant_token, "GET", "/admin/v1/operator-tokens").status,
        401,
        "a tenant token is no operator token"
    );
    let bad = server.admin_call(
        "POST",
        "/admin/v1/operator-tokens",
        Some(&json!({"name": ""})),
    );
    assert_eq!(
        bad.status, 401,
        "the revoked bootstrap token cannot create more"
    );
}

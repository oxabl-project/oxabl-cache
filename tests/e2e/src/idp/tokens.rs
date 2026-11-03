//! ID tokens shaped like those of GitHub Actions, GitLab CI and a generic issuer.

use super::keys::SigningKey;
use aws_lc_rs::hmac;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

fn times() -> Value {
    let now = now();
    json!({"iat": now, "nbf": now, "exp": now + 600})
}

fn merged(mut base: Value, extra: Value) -> Value {
    base.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    base
}

/// A GitHub Actions token.
pub fn github(
    issuer: &str,
    audience: &str,
    repository: &str,
    repository_id: &str,
    git_ref: &str,
    environment: &str,
) -> Value {
    let owner = repository.split('/').next().unwrap_or(repository);
    merged(
        times(),
        json!({
            "iss": issuer, "aud": audience,
            "sub": format!("repo:{repository}:environment:{environment}"),
            "repository": repository, "repository_id": repository_id,
            "repository_owner": owner, "repository_owner_id": "5550001",
            "ref": git_ref, "environment": environment, "event_name": "push",
            "workflow": "build", "actor": "someone", "runner_environment": "github-hosted",
        }),
    )
}

/// A GitLab CI token.
pub fn gitlab(
    issuer: &str,
    audience: &str,
    project_path: &str,
    project_id: &str,
    git_ref: &str,
) -> Value {
    merged(
        times(),
        json!({
            "iss": issuer, "aud": audience,
            "sub": format!("project_path:{project_path}:ref_type:branch:ref:{git_ref}"),
            "project_path": project_path, "project_id": project_id,
            "namespace_id": "7770001", "namespace_path": "group",
            "ref": git_ref, "ref_type": "branch", "ref_protected": "true",
            "pipeline_source": "push",
        }),
    )
}

/// A token of any other issuer: a subject and little else.
pub fn generic(issuer: &str, audience: &str, subject: &str) -> Value {
    merged(
        times(),
        json!({"iss": issuer, "aud": audience, "sub": subject}),
    )
}

fn segment(value: &Value) -> String {
    URL_SAFE_NO_PAD.encode(value.to_string())
}

/// A correctly signed token.
pub fn sign(key: &SigningKey, claims: &Value) -> String {
    let header = json!({"alg": key.alg_name(), "typ": "JWT", "kid": key.kid});
    let signed = format!("{}.{}", segment(&header), segment(claims));
    let signature = URL_SAFE_NO_PAD.encode(key.sign(signed.as_bytes()));
    format!("{signed}.{signature}")
}

/// A token whose header names `kid` but which `key` signs.
pub fn sign_as(key: &SigningKey, kid: &str, claims: &Value) -> String {
    let header = json!({"alg": key.alg_name(), "typ": "JWT", "kid": kid});
    let signed = format!("{}.{}", segment(&header), segment(claims));
    let signature = URL_SAFE_NO_PAD.encode(key.sign(signed.as_bytes()));
    format!("{signed}.{signature}")
}

/// `alg: none` with an empty signature.
pub fn unsigned(kid: &str, claims: &Value) -> String {
    let header = json!({"alg": "none", "typ": "JWT", "kid": kid});
    format!("{}.{}.", segment(&header), segment(claims))
}

/// `alg: HS256` keyed with the issuer's public key bytes.
pub fn hmac_forged(key: &SigningKey, claims: &Value) -> String {
    let header = json!({"alg": "HS256", "typ": "JWT", "kid": key.kid});
    let signed = format!("{}.{}", segment(&header), segment(claims));
    let mac = hmac::Key::new(hmac::HMAC_SHA256, &key.public_bytes());
    let signature = URL_SAFE_NO_PAD.encode(hmac::sign(&mac, signed.as_bytes()).as_ref());
    format!("{signed}.{signature}")
}

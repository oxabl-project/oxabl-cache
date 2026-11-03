//! The real `oxabl-cache-admin` binary against the real `oxabl-cache-server` binary.

mod credentials;
mod export_order;
mod support;
mod tenants;
mod tls;
mod usage;

use reqwest::blocking::{Client, Response};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use support::admin::run;
use support::server::Server;

pub fn month_now() -> String {
    let days = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}")
}

pub fn owner_key() -> String {
    let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
    key.verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn call(base: &str, token: &str, method: &str, path: &str, body: Option<&Value>) -> Response {
    let method = reqwest::Method::from_bytes(method.as_bytes()).expect("method");
    let mut request = Client::new()
        .request(method, format!("{base}{path}"))
        .bearer_auth(token);
    if let Some(body) = body {
        request = request.json(body);
    }
    request.send().expect("request")
}

/// A tenant with a cache and a `read` token, made through the admin CLI and the tenant API.
pub struct Tenant {
    pub id: String,
    pub cache: String,
    pub read_token: String,
    pub admin_token: String,
}

impl Tenant {
    pub fn create(server: &Server, name: &str) -> Self {
        let created = run(server, &["--json", "tenant", "create", name]);
        assert_eq!(created.code(), 0, "{}", created.stderr());
        let id = created.json()["id"].as_str().expect("id").to_owned();
        let minted = run(server, &["--json", "token", "create", "--tenant", &id]);
        assert_eq!(minted.code(), 0, "{}", minted.stderr());
        let admin_token = minted.json()["token"].as_str().expect("token").to_owned();
        let public = server.public_url();
        let cache = call(
            &public,
            &admin_token,
            "POST",
            "/v1/tenant/caches",
            Some(&json!({"display_name": "main", "owner_keys": [owner_key()]})),
        )
        .json::<Value>()
        .expect("json")["id"]
            .as_str()
            .expect("cache id")
            .to_owned();
        let read = call(
            &public,
            &admin_token,
            "POST",
            "/v1/tenant/tokens",
            Some(&json!({"name": "reader", "scopes": ["read"], "caches": [cache]})),
        )
        .json::<Value>()
        .expect("json");
        Self {
            id,
            cache,
            read_token: read["token"].as_str().expect("token").to_owned(),
            admin_token,
        }
    }

    pub fn fetch_cache_info(&self, server: &Server) -> Response {
        call(
            &server.public_url(),
            &self.read_token,
            "GET",
            &format!("/v1/caches/{}", self.cache),
            None,
        )
    }

    /// A plan-mode lookup of `count` random ids.
    pub fn lookup(&self, server: &Server, count: usize) -> Response {
        let ids = (0..count)
            .map(|_| {
                let mut id = [0u8; 32];
                getrandom::fill(&mut id).expect("randomness");
                oxabl_cache_format::LookupId(id)
            })
            .collect();
        let frame = oxabl_cache_format::LookupRequest {
            flags: oxabl_cache_format::LookupFlags {
                records: false,
                urls: false,
            },
            namespaces: oxabl_cache_format::Namespaces {
                trusted: true,
                personal: false,
            },
            ids,
        }
        .encode()
        .expect("lookup frame");
        Client::new()
            .post(format!(
                "{}/v1/caches/{}/lookup",
                server.public_url(),
                self.cache
            ))
            .bearer_auth(&self.read_token)
            .header("content-type", "application/vnd.oxabl-cache.v1")
            .body(frame)
            .send()
            .expect("lookup")
    }
}

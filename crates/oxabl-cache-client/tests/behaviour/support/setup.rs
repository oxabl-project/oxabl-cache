//! Tenant, cache, token and roster setup over the server's administration API.

use oxabl_cache_crypto::{Identity, MemberEntry, RosterDraft};
use oxabl_cache_format::{CacheId, Role};
use serde_json::{Value, json};

pub struct Setup {
    http: reqwest::Client,
    public: String,
    admin: String,
    operator: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn expect(response: reqwest::Response, status: u16) -> Value {
    let got = response.status().as_u16();
    let body = response.text().await.expect("body");
    assert_eq!(got, status, "unexpected status, body: {body}");
    serde_json::from_str(&body).unwrap_or(Value::Null)
}

impl Setup {
    pub fn new(public: &str, admin: &str, operator: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            public: format!("http://{public}"),
            admin: format!("http://{admin}"),
            operator: operator.to_owned(),
        }
    }

    pub async fn tenant(&self, name: &str) -> (String, String) {
        let created = self
            .http
            .post(format!("{}/admin/v1/tenants", self.admin))
            .bearer_auth(&self.operator)
            .json(&json!({"name": name}))
            .send()
            .await
            .expect("create tenant");
        let tenant = expect(created, 201).await["id"]
            .as_str()
            .expect("id")
            .to_owned();
        let token = self
            .http
            .post(format!("{}/admin/v1/tenants/{tenant}/tokens", self.admin))
            .bearer_auth(&self.operator)
            .json(&json!({"name": "admin"}))
            .send()
            .await
            .expect("admin token");
        let token = expect(token, 201).await["token"]
            .as_str()
            .expect("token")
            .to_owned();
        (tenant, token)
    }

    pub async fn cache(&self, admin: &str, name: &str, owner_key: &[u8; 32]) -> String {
        self.cache_with_quota(admin, name, owner_key, None).await
    }

    pub async fn cache_with_quota(
        &self,
        admin: &str,
        name: &str,
        owner_key: &[u8; 32],
        hard_bytes: Option<u64>,
    ) -> String {
        let mut body = json!({"display_name": name, "owner_keys": [hex(owner_key)]});
        if let Some(bytes) = hard_bytes {
            body["hard_bytes"] = bytes.into();
        }
        let created = self
            .http
            .post(format!("{}/v1/tenant/caches", self.public))
            .bearer_auth(admin)
            .json(&body)
            .send()
            .await
            .expect("create cache");
        expect(created, 201).await["id"]
            .as_str()
            .expect("id")
            .to_owned()
    }

    pub async fn token(&self, admin: &str, name: &str, scopes: &[&str], cache: &str) -> String {
        let created = self
            .http
            .post(format!("{}/v1/tenant/tokens", self.public))
            .bearer_auth(admin)
            .json(&json!({"name": name, "scopes": scopes, "caches": [cache]}))
            .send()
            .await
            .expect("create token");
        expect(created, 201).await["token"]
            .as_str()
            .expect("token")
            .to_owned()
    }

    pub async fn put_roster(&self, token: &str, cache: &str, version: u64, bytes: Vec<u8>) {
        let put = self
            .http
            .put(format!("{}/v1/caches/{cache}/roster", self.public))
            .bearer_auth(token)
            .header("If-Match", format!("\"v{}\"", version - 1))
            .header("Content-Type", "application/vnd.oxabl-cache.v1")
            .body(bytes)
            .send()
            .await
            .expect("put roster");
        assert_eq!(
            put.status().as_u16(),
            204,
            "{}",
            put.text().await.unwrap_or_default()
        );
    }
}

/// A roster member: a label, a role and the identity that holds its keys.
pub fn member(label: &str, role: Role, identity: &Identity) -> MemberEntry {
    MemberEntry::from_descriptor(label, role, &identity.descriptor().expect("descriptor"))
        .expect("member")
}

/// The first roster of a cache, signed by `owner`.
pub fn genesis(cache: &str, members: Vec<MemberEntry>, owner: &Identity) -> (RosterDraft, Vec<u8>) {
    let cache_id = CacheId::from_text(cache).expect("cache id");
    let draft = RosterDraft::genesis(cache_id, members).expect("genesis");
    let bytes = draft.sign(&[owner]).expect("sign");
    (draft, bytes)
}

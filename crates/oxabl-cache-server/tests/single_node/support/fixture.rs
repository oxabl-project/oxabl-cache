//! A tenant with a cache and tokens of each role, on a running server.

use super::frames::{self, Entry, Pack};
use super::http::{Response, request, split_url};
use super::server::{Client, Options, Server};
use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use oxabl_cache_format as format;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub struct World {
    pub server: Server,
    pub tenant: String,
    pub admin: Client,
    pub cache: String,
    pub cache_bytes: [u8; 16],
    pub ci: Client,
    pub dev: Client,
    pub reader: Client,
}

pub fn token(admin: &Client, name: &str, scopes: &[&str], caches: &[&str]) -> Value {
    let created = admin.json(
        "POST",
        "/v1/tenant/tokens",
        Some(&json!({"name": name, "scopes": scopes, "caches": caches})),
    );
    assert_eq!(
        created.status,
        201,
        "{}",
        String::from_utf8_lossy(&created.body)
    );
    created.json()
}

pub fn owner() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

pub fn owner_key() -> String {
    hex(&owner().verifying_key().to_bytes())
}

pub fn create_cache(admin: &Client, name: &str, extra: Value) -> String {
    let mut body = json!({"display_name": name, "owner_keys": [owner_key()]});
    if let (Some(fields), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        fields.extend(extra.clone());
    }
    let created = admin.json("POST", "/v1/tenant/caches", Some(&body));
    assert_eq!(
        created.status,
        201,
        "{}",
        String::from_utf8_lossy(&created.body)
    );
    created.json()["id"].as_str().expect("id").to_owned()
}

impl World {
    pub fn start(name: &str, options: Options) -> Self {
        Self::build(name, options, json!({}), json!({}))
    }

    pub fn build(name: &str, options: Options, tenant_extra: Value, cache_extra: Value) -> Self {
        let server = Server::start(name, options);
        let tenant = server.create_tenant("acme", tenant_extra);
        let admin = server.tenant_admin(&tenant);
        let cache = create_cache(&admin, "codebase", cache_extra);
        let cache_bytes = format::CacheId::from_text(&cache).expect("cache id").0;
        let make = |name: &str, scopes: &[&str]| {
            let created = token(&admin, name, scopes, &[&cache]);
            admin.with_token(created["token"].as_str().expect("token"))
        };
        let ci = make("ci", &["read", "write:trusted"]);
        let dev = make("dev", &["read", "write:personal"]);
        let reader = make("reader", &["read"]);
        Self {
            server,
            tenant,
            admin,
            cache,
            cache_bytes,
            ci,
            dev,
            reader,
        }
    }

    /// Points every client at the server's current address, after a restart.
    pub fn reconnect(&mut self) {
        for client in [
            &mut self.admin,
            &mut self.ci,
            &mut self.dev,
            &mut self.reader,
        ] {
            client.addr.clone_from(&self.server.public);
        }
    }

    pub fn path(&self, suffix: &str) -> String {
        format!("/v1/caches/{}{suffix}", self.cache)
    }

    pub fn info(&self, client: &Client) -> Value {
        let info = client.json("GET", &self.path(""), None);
        assert_eq!(info.status, 200);
        info.json()
    }

    /// Reserves a pack that is never uploaded: its digest is a placeholder.
    pub fn reserve(&self, client: &Client, namespace: &str, size: usize) -> Response {
        self.reserve_digest(client, namespace, size, &Sha256::digest(b""))
    }

    /// Reserves a pack of exactly `bytes`.
    pub fn reserve_bytes(&self, client: &Client, namespace: &str, bytes: &[u8]) -> Response {
        self.reserve_digest(client, namespace, bytes.len(), &Sha256::digest(bytes))
    }

    fn reserve_digest(
        &self,
        client: &Client,
        namespace: &str,
        size: usize,
        digest: &[u8],
    ) -> Response {
        client.json(
            "POST",
            &self.path("/uploads"),
            Some(&json!({
                "namespace": namespace,
                "packs": [{
                    "client_ref": hex(&frames::random(16)),
                    "size": size,
                    "sha256": base64::engine::general_purpose::STANDARD.encode(digest),
                }],
            })),
        )
    }

    /// PUTs `bytes` to a reserved pack's URL with the headers the slot names.
    pub fn put_slot(&self, slot: &Value, bytes: &[u8]) -> Response {
        let (addr, path) = split_url(slot["url"].as_str().expect("url"));
        let headers: Vec<(&str, String)> = slot["headers"]
            .as_object()
            .expect("headers")
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("content-length"))
            .map(|(name, value)| (name.as_str(), value.as_str().expect("header").to_owned()))
            .collect();
        request(&addr, "PUT", &path, &headers, bytes)
    }

    pub fn get_object(&self, url: &str, range: Option<&str>) -> Response {
        let (addr, path) = split_url(url);
        let headers: Vec<(&str, String)> =
            range.map(|r| ("Range", r.to_owned())).into_iter().collect();
        request(&addr, "GET", &path, &headers, b"")
    }

    pub fn commit(&self, client: &Client, pack_id: &str, frame: &[u8]) -> Response {
        client.binary(
            "POST",
            &self.path(&format!("/packs/{pack_id}/commit")),
            frame,
        )
    }

    /// Reserves, uploads and commits a pack; returns its id and the entry statuses.
    pub fn publish(
        &self,
        client: &Client,
        ns: format::Namespace,
        pack: &Pack,
        entries: &[Entry],
    ) -> (String, Vec<format::CommitStatus>) {
        let namespace = match ns {
            format::Namespace::Trusted => "trusted",
            format::Namespace::Personal => "personal",
        };
        let reserved = self.reserve_bytes(client, namespace, &pack.bytes);
        assert_eq!(
            reserved.status,
            200,
            "{}",
            String::from_utf8_lossy(&reserved.body)
        );
        let slot = reserved.json()["packs"][0].clone();
        assert_eq!(slot["status"], "ok", "{slot}");
        let put = self.put_slot(&slot, &pack.bytes);
        assert_eq!(put.status, 200, "{}", String::from_utf8_lossy(&put.body));
        let pack_id = slot["pack_id"].as_str().expect("pack id").to_owned();
        let committed = self.commit(client, &pack_id, &frames::commit(ns, pack, entries));
        assert_eq!(
            committed.status,
            200,
            "{}",
            String::from_utf8_lossy(&committed.body)
        );
        (pack_id, frames::statuses(&committed.body))
    }

    /// Looks up ids in the trusted namespace in fetch mode with URLs.
    pub fn fetch(&self, client: &Client, ids: &[[u8; 32]]) -> (Vec<u8>, format::LookupShape) {
        let (frame, shape) = frames::lookup(ids, true, true, true, false);
        let answered = client.binary("POST", &self.path("/lookup"), &frame);
        assert_eq!(
            answered.status,
            200,
            "{}",
            String::from_utf8_lossy(&answered.body)
        );
        (answered.body, shape)
    }

    pub fn object_exists(&self, pack_id: &str) -> bool {
        self.server.objects.exists(&self.cache, pack_id)
    }

    pub fn read_object(&self, pack_id: &str) -> Vec<u8> {
        self.server.objects.read(&self.cache, pack_id)
    }

    /// Replaces a stored pack's bytes behind the server's back.
    pub fn overwrite_object(&self, pack_id: &str, bytes: Vec<u8>) {
        self.server.objects.overwrite(&self.cache, pack_id, bytes);
    }

    /// Flips one bit of the stored pack at `offset`.
    pub fn flip_stored_bit(&self, pack_id: &str, offset: usize, mask: u8) {
        let mut object = self.read_object(pack_id);
        object[offset] ^= mask;
        self.overwrite_object(pack_id, object);
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Polls `condition` every 100 ms until it holds; false once `timeout` passes.
pub fn wait_until(timeout: std::time::Duration, mut condition: impl FnMut() -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        if condition() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

/// An `OXRS` roster with the owner and `others` as members, signed by `signer`.
pub fn roster_bytes(
    world: &World,
    others: &[(format::Role, [u8; 32])],
    version: u64,
    epoch: u32,
    signer: &SigningKey,
) -> Vec<u8> {
    let mut members = vec![format::Member {
        label: "owner",
        role: format::Role::Owner,
        sign_keys: vec![owner().verifying_key().to_bytes()],
        age_recipients: Vec::new(),
    }];
    members.extend(others.iter().map(|(role, key)| format::Member {
        label: "member",
        role: *role,
        sign_keys: vec![*key],
        age_recipients: Vec::new(),
    }));
    let unsigned = format::Roster {
        cache_id: format::CacheId(world.cache_bytes),
        version,
        epoch,
        members,
        keyring: b"opaque age file",
        signatures: Vec::new(),
    };
    let mut message = b"oxabl-cache v1 roster".to_vec();
    message.extend_from_slice(&unsigned.signed_bytes().expect("signed bytes"));
    let signature = format::RosterSignature {
        owner_key: signer.verifying_key().to_bytes(),
        signature: signer.sign(&message).to_bytes(),
    };
    format::Roster {
        signatures: vec![signature],
        ..unsigned
    }
    .encode()
    .expect("roster")
}

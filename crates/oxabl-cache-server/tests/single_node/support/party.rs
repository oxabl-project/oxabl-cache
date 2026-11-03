//! A second cache on a running server: in the world's tenant, or in a tenant of its own.

use super::fixture::{World, create_cache, token};
use super::frames::{self, Entry};
use super::server::Client;
use oxabl_cache_format as format;
use serde_json::json;

pub struct Party {
    pub tenant: String,
    pub admin: Client,
    pub cache: String,
    pub cache_bytes: [u8; 16],
    pub ci: Client,
}

impl Party {
    /// A new cache in the tenant `admin` administers.
    pub fn on(tenant: &str, admin: &Client, name: &str) -> Self {
        let cache = create_cache(admin, name, json!({}));
        let cache_bytes = format::CacheId::from_text(&cache).expect("cache id").0;
        let ci = token(admin, "ci", &["read", "write:trusted"], &[&cache]);
        Self {
            tenant: tenant.to_owned(),
            admin: admin.clone(),
            cache,
            cache_bytes,
            ci: admin.with_token(ci["token"].as_str().expect("token")),
        }
    }

    /// A new tenant with one cache, on the server of `world`.
    pub fn in_new_tenant(world: &World, name: &str) -> Self {
        let tenant = world.server.create_tenant(name, json!({}));
        let admin = world.server.tenant_admin(&tenant);
        Self::on(&tenant, &admin, "codebase")
    }

    /// Points the party's clients at the server's current address, after a restart.
    pub fn reconnect(&mut self, world: &World) {
        for client in [&mut self.admin, &mut self.ci] {
            client.addr.clone_from(&world.server.public);
        }
    }

    pub fn path(&self, suffix: &str) -> String {
        format!("/v1/caches/{}{suffix}", self.cache)
    }

    /// Publishes one pack of `entries` artifact sets and returns its id and the lookup ids.
    pub fn publish(&self, world: &World, entries: usize) -> (String, Vec<[u8; 32]>) {
        let pack = frames::pack(self.cache_bytes, &vec![3_000; entries]);
        let keys: Vec<[u8; 32]> = (0..entries).map(|_| frames::lookup_id()).collect();
        let list: Vec<Entry> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| Entry::new(*key, index as u32))
            .collect();
        let reserved = world.reserve_bytes_in(&self.ci, &self.cache, "trusted", &pack.bytes);
        assert_eq!(
            reserved.status,
            200,
            "{}",
            String::from_utf8_lossy(&reserved.body)
        );
        let slot = reserved.json()["packs"][0].clone();
        assert_eq!(slot["status"], "ok", "{slot}");
        assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
        let pack_id = slot["pack_id"].as_str().expect("pack id").to_owned();
        let committed = self.ci.binary(
            "POST",
            &self.path(&format!("/packs/{pack_id}/commit")),
            &frames::commit(format::Namespace::Trusted, &pack, &list),
        );
        assert_eq!(
            committed.status,
            200,
            "{}",
            String::from_utf8_lossy(&committed.body)
        );
        (pack_id, keys)
    }

    /// How many of `ids` a plan-mode lookup finds, or the refusing status.
    pub fn hits(&self, ids: &[[u8; 32]]) -> Result<usize, u16> {
        let (frame, shape) = frames::lookup(ids, false, false, true, false);
        let answered = self.ci.binary("POST", &self.path("/lookup"), &frame);
        if answered.status != 200 {
            return Err(answered.status);
        }
        Ok(format::LookupResponse::decode(&answered.body, &shape)
            .expect("OXLR")
            .hits
            .len())
    }
}

//! Advisory locks that serialise writers of overlapping scopes.
//!
//! Lock order is fixed: the global lock, then the tenant lock, then the cache
//! lock. Scoped transactions hold the global lock shared and a global
//! transaction holds it exclusive, so a global transaction excludes everything
//! while transactions of different caches run side by side. Keys are 64-bit
//! hashes; a collision only serialises two scopes that could have run together.

use oxabl_cache_core::ports::TxScope;

fn key(domain: &str, id: &[u8]) -> i64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"oxabl-cache advisory lock ");
    hasher.update(domain.as_bytes());
    hasher.update(id);
    let hash = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash.as_bytes()[..8]);
    i64::from_le_bytes(bytes)
}

/// The lock key that serialises schema migrations.
pub(crate) fn migration_key() -> i64 {
    key("migration", &[])
}

/// The lock key of the garbage-collection leader.
pub(crate) fn leader_key() -> i64 {
    key("leader", &[])
}

/// SQL statements, in lock order, that take the locks of a write transaction.
pub(crate) fn acquire(scope: TxScope) -> String {
    let global = key("global", &[]);
    let shared = format!("SELECT pg_advisory_xact_lock_shared({global})");
    let exclusive =
        |domain: &str, id: &[u8]| format!("SELECT pg_advisory_xact_lock({})", key(domain, id));
    match scope {
        TxScope::Global => format!("SELECT pg_advisory_xact_lock({global})"),
        TxScope::Independent => shared,
        TxScope::Ledger => format!("{shared}; {}", exclusive("ledger", &[])),
        TxScope::Tenant(tenant) => {
            format!("{shared}; {}", exclusive("tenant", tenant.as_bytes()))
        }
        TxScope::Cache(cache) => format!("{shared}; {}", exclusive("cache", cache.as_bytes())),
        TxScope::TenantCache(tenant, cache) => format!(
            "{shared}; {}; {}",
            exclusive("tenant", tenant.as_bytes()),
            exclusive("cache", cache.as_bytes())
        ),
    }
}

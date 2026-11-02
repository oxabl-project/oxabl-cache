//! Quota admission of one pack reservation, evicting personal entries for CI.

use crate::config::Config;
use crate::domain::cache::Cache;
use crate::domain::namespace::{Namespace, NamespaceKind};
use crate::error::{Error, Result};
use crate::ports::MetaTx;
use crate::ports::metadata::EvictionScope;
use crate::usecases::entry_removal::remove_entries;

/// Whether a pack of `size` bytes fits. A trusted pack first evicts personal
/// entries, oldest first, until it fits; it is admitted over the hard quota only
/// while trusted live and pending bytes alone stay within it. Evictions are part
/// of the caller's transaction.
pub(crate) fn admit(
    tx: &mut dyn MetaTx,
    config: &Config,
    cache: &Cache,
    kind: NamespaceKind,
    namespace: Namespace,
    size: u64,
    now: i64,
) -> Result<bool> {
    let totals = tx.pack_totals(&cache.id)?;
    let ceiling = cache
        .hard_bytes
        .saturating_mul(config.stored_ceiling_factor);
    if totals.stored_bytes.saturating_add(size) > ceiling {
        return Ok(false);
    }
    let tenant = tx.tenant(&cache.tenant)?.ok_or(Error::NotFound)?;
    let over = |tx: &mut dyn MetaTx| -> Result<u64> {
        let current = tx.cache(&cache.tenant, &cache.id)?.ok_or(Error::NotFound)?;
        let by_cache =
            (current.live_bytes + current.pending_bytes + size).saturating_sub(current.hard_bytes);
        let by_tenant = match tenant.hard_bytes {
            Some(limit) => {
                let (live, pending) = tx.tenant_bytes(&cache.tenant)?;
                (live + pending + size).saturating_sub(limit)
            }
            None => 0,
        };
        Ok(by_cache.max(by_tenant))
    };
    match kind {
        NamespaceKind::Personal => {
            let cap = cache.hard_bytes / 1000 * config.personal_cap_permille;
            let held = tx.namespace_bytes(&cache.id, &namespace.key())?;
            Ok(held.saturating_add(size) <= cap && over(tx)? == 0)
        }
        NamespaceKind::Trusted => {
            let trusted = tx.namespace_bytes(&cache.id, &Namespace::Trusted.key())?;
            if trusted.saturating_add(size) > cache.hard_bytes {
                return Ok(false);
            }
            let excess = over(tx)?;
            if excess > 0 {
                evict_personal(tx, config, cache, excess, now)?;
            }
            Ok(tenant.hard_bytes.is_none() || over(tx)? == 0)
        }
    }
}

fn evict_personal(
    tx: &mut dyn MetaTx,
    config: &Config,
    cache: &Cache,
    excess: u64,
    now: i64,
) -> Result<()> {
    let mut freed = 0u64;
    while freed < excess {
        let batch = tx.oldest_entries(&cache.id, EvictionScope::Personal, config.gc_batch)?;
        if batch.is_empty() {
            break;
        }
        let removal = remove_entries(tx, &cache.id, &batch, now, |released| {
            freed + released >= excess
        })?;
        if removal.entries == 0 {
            break;
        }
        freed += removal.freed_bytes;
    }
    Ok(())
}

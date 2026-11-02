//! Resolving a cache for a caller: tenant scope first, then the scope check.

use crate::domain::cache::Cache;
use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::error::{Error, Result};
use crate::ids::CacheId;
use crate::ports::MetaTx;

/// The cache if it belongs to the caller's tenant (else `NotFound`) and the
/// caller holds `scope` on it (else `Forbidden`).
pub(crate) fn scoped_cache(
    tx: &mut dyn MetaTx,
    caller: &Caller,
    id: &CacheId,
    scope: Scope,
) -> Result<Cache> {
    let cache = tx.cache(&caller.tenant, id)?.ok_or(Error::NotFound)?;
    caller.require(id, scope)?;
    Ok(cache)
}

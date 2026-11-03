//! Row mapping shared by the pack files.

use crate::tx::RowExt;
use oxabl_cache_core::domain::pack::{Pack, PackState};
use oxabl_cache_core::ids::{CacheId, ClientRef, PackId, PrincipalId};
use oxabl_cache_core::{Error, Result};
use tokio_postgres::Row;

pub(in crate::tx) const PACK_COLUMNS: &str = "cache_id, pack_id, ns, uploader, client_ref, size, etag, live_bytes, \
     state, lease_until, created_at, dead_since, commit_digest, commit_statuses";

pub(in crate::tx) fn state_code(state: PackState) -> i64 {
    match state {
        PackState::Pending => 0,
        PackState::Live => 1,
        PackState::Deleting => 2,
    }
}

pub(in crate::tx) fn state_of(code: i64) -> Result<PackState> {
    match code {
        0 => Ok(PackState::Pending),
        1 => Ok(PackState::Live),
        2 => Ok(PackState::Deleting),
        _ => Err(Error::internal("stored pack state is unknown")),
    }
}

pub(in crate::tx) fn pack_of(row: &Row) -> Result<Pack> {
    Ok(Pack {
        cache: CacheId::from_bytes(row.blob(0)?),
        id: PackId::from_bytes(row.blob(1)?),
        namespace: row.blob(2)?,
        uploader: PrincipalId::from_bytes(row.blob(3)?),
        client_ref: ClientRef::from_bytes(row.blob(4)?),
        size: row.unsigned(5)?,
        etag: row.opt_text(6)?,
        live_bytes: row.unsigned(7)?,
        state: state_of(row.int(8)?)?,
        lease_until: row.int(9)?,
        created_at: row.int(10)?,
        dead_since: row.opt_int(11)?,
        commit_digest: row.opt_blob(12)?,
        commit_statuses: row.opt_bytes(13)?,
    })
}

pub(in crate::tx) fn packs(rows: &[Row]) -> Result<Vec<Pack>> {
    rows.iter().map(pack_of).collect()
}

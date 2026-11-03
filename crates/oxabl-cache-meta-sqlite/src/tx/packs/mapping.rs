//! Row mapping shared by the pack files.

use crate::db::{DbResult, RowExt};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::pack::{Pack, PackState};
use oxabl_cache_core::ids::{CacheId, ClientRef, PackId, PrincipalId};
use rusqlite::Row;

pub(in crate::tx) const PACK_COLUMNS: &str = "cache_id, pack_id, ns, uploader, client_ref, size, etag, live_bytes, \
     state, lease_until, created_at, dead_since, commit_digest, commit_statuses";

pub(in crate::tx) fn state_code(state: PackState) -> i64 {
    match state {
        PackState::Pending => 0,
        PackState::Live => 1,
        PackState::Deleting => 2,
    }
}

pub(in crate::tx) fn state_of(code: i64) -> Option<PackState> {
    match code {
        0 => Some(PackState::Pending),
        1 => Some(PackState::Live),
        2 => Some(PackState::Deleting),
        _ => None,
    }
}

pub(in crate::tx) fn pack_of(row: &Row<'_>) -> rusqlite::Result<Pack> {
    let state: i64 = row.get(8)?;
    Ok(Pack {
        cache: CacheId::from_bytes(row.blob(0)?),
        id: PackId::from_bytes(row.blob(1)?),
        namespace: row.blob(2)?,
        uploader: PrincipalId::from_bytes(row.blob(3)?),
        client_ref: ClientRef::from_bytes(row.blob(4)?),
        size: row.unsigned(5)?,
        etag: row.get(6)?,
        live_bytes: row.unsigned(7)?,
        state: state_of(state).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Integer,
                "stored pack state is unknown".into(),
            )
        })?,
        lease_until: row.get(9)?,
        created_at: row.get(10)?,
        dead_since: row.get(11)?,
        commit_digest: row.opt_blob(12)?,
        commit_statuses: row.get(13)?,
    })
}

pub(in crate::tx) fn collect(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<Pack>>,
) -> Result<Vec<Pack>> {
    rows.collect::<rusqlite::Result<_>>().db()
}

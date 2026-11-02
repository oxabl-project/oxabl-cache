//! `GET /v1/caches/{cache}`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::json_response;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::json;
use std::sync::Arc;

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let info = app.engine.cache_info(&auth.caller, cache).await?;
    Ok(json_response(
        StatusCode::OK,
        &json!({
            "id": info.id.to_string(),
            "display_name": info.display_name,
            "epoch": info.epoch,
            "roster_version": info.roster_version,
            "dictionary_version": info.dictionary_version,
            "store": {
                "kind": info.store.kind,
                "if_none_match": info.store.if_none_match,
                "checksum_sha256": info.store.checksum_sha256,
            },
            "limits": {
                "lookup_ids": info.limits.lookup_ids,
                "commit_entries": info.limits.commit_entries,
                "pack_bytes": info.limits.pack_bytes,
            },
            "quota": {
                "hard_bytes": info.quota.hard_bytes,
                "live_bytes": info.quota.live_bytes,
                "pending_bytes": info.quota.pending_bytes,
                "stored_bytes": info.quota.stored_bytes,
                "reclaimable_bytes": info.quota.reclaimable_bytes,
            },
        }),
    ))
}

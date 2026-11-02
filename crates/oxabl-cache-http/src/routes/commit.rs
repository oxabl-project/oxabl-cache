//! `POST /v1/caches/{cache}/packs/{pack_id}/commit`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::body::read_limited;
use crate::error::ApiError;
use crate::frames::{encode_statuses, parse_commit};
use crate::path::pack_id;
use crate::routes::versioned::{binary, require_binary};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use oxabl_cache_core::domain::entry::EntryStatus;
use std::sync::Arc;

const COMMIT_BODY_BYTES: usize = 9 * 1024 * 1024;

fn status_label(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Committed => "committed",
        EntryStatus::Exists => "exists",
        EntryStatus::Invalid => "invalid",
        EntryStatus::MissingDictionary => "missing_dictionary",
    }
}

pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path((cache, pack)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let pack = pack_id(&pack)?;
    require_binary(&headers)?;
    let bytes = read_limited(body, COMMIT_BODY_BYTES).await?;
    auth.ctx.count("bytes_in", bytes.len() as u64);
    let request = parse_commit(cache, pack, &bytes)?;
    let statuses = app.engine.commit_pack(&auth.caller, request).await?;
    for status in &statuses {
        app.metrics.inc(
            "oxabl_cache_commit_entries_total",
            &[("status", status_label(*status))],
            1,
        );
    }
    auth.ctx.count("entries", statuses.len() as u64);
    Ok(binary(encode_statuses(&statuses)?, None))
}

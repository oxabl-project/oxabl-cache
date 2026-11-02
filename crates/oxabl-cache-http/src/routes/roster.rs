//! `GET|PUT /v1/caches/{cache}/roster`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::body::read_limited;
use crate::error::ApiError;
use crate::frames::parse_roster;
use crate::json::empty;
use crate::routes::versioned::{binary, if_match};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use oxabl_cache_format::limits::ROSTER_BYTES;
use std::sync::Arc;

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let stored = app.engine.get_roster(&auth.caller, cache).await?;
    Ok(binary(stored.bytes, Some(stored.version)))
}

pub(crate) async fn put<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let expected = if_match(&headers)?;
    let bytes = read_limited(body, ROSTER_BYTES).await?;
    auth.ctx.count("bytes_in", bytes.len() as u64);
    let roster = parse_roster(cache, &bytes)?;
    app.engine
        .put_roster(&auth.caller, cache, expected, roster)
        .await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

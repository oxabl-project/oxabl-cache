//! `GET|PUT /v1/caches/{cache}/dictionary`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::body::read_limited;
use crate::error::ApiError;
use crate::frames::parse_pointer;
use crate::json::empty;
use crate::routes::versioned::{binary, if_match};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use std::sync::Arc;

const POINTER_BODY_BYTES: usize = 4096;

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let stored = app.engine.get_dictionary(&auth.caller, cache).await?;
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
    let bytes = read_limited(body, POINTER_BODY_BYTES).await?;
    let pointer = parse_pointer(cache, &bytes)?;
    app.engine
        .put_dictionary(&auth.caller, cache, expected, pointer)
        .await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

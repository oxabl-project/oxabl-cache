//! `GET|PUT /v1/tenant/caches/{cache}/owner-keys`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{empty, json_response, read_json};
use crate::path::public_key;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::encoding::hex_encode;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerKeys {
    owner_keys: Vec<String>,
}

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let keys = app.engine.get_owner_keys(&auth.caller, cache).await?;
    let keys: Vec<String> = keys.iter().map(|key| hex_encode(key)).collect();
    Ok(json_response(
        StatusCode::OK,
        &json!({ "owner_keys": keys }),
    ))
}

pub(crate) async fn put<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let body: OwnerKeys = read_json(body).await?;
    let keys = body
        .owner_keys
        .iter()
        .map(|text| public_key(text))
        .collect::<Result<Vec<_>, _>>()?;
    app.engine.set_owner_keys(&auth.caller, cache, keys).await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

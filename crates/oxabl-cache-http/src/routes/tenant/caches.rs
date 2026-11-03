//! `GET|POST /v1/tenant/caches`, `DELETE /v1/tenant/caches/{cache}`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{empty, json_response, read_json};
use crate::path::public_key;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::domain::cache::Cache;
use oxabl_cache_core::usecases::CacheSpec;
use oxabl_cache_core::{Conflict, Error};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewCache {
    display_name: String,
    retention_days: Option<u32>,
    hard_bytes: Option<u64>,
    owner_keys: Vec<String>,
}

fn cache_json(cache: &Cache) -> Value {
    json!({
        "id": cache.id.to_string(),
        "display_name": cache.display_name,
        "retention_days": cache.retention_days,
        "hard_bytes": cache.hard_bytes,
        "epoch": cache.epoch,
        "roster_version": cache.roster_version,
        "live_bytes": cache.live_bytes,
        "pending_bytes": cache.pending_bytes,
        "created_at": cache.created_at,
    })
}

pub(crate) async fn list<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
) -> Result<Response, ApiError> {
    let caches = app.engine.list_caches(&auth.caller).await?;
    let caches: Vec<Value> = caches.iter().map(cache_json).collect();
    Ok(json_response(StatusCode::OK, &json!({ "caches": caches })))
}

/// Creates a cache. A name is the natural key: the identical request again is a 200.
pub(crate) async fn create<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    let body: NewCache = read_json(body).await?;
    let mut keys = body
        .owner_keys
        .iter()
        .map(|text| public_key(text))
        .collect::<Result<Vec<_>, _>>()?;
    keys.sort_unstable();
    keys.dedup();
    let spec = CacheSpec {
        display_name: body.display_name.clone(),
        retention_days: body.retention_days,
        hard_bytes: body.hard_bytes,
        owner_keys: keys.clone(),
    };
    match app.engine.create_cache(&auth.caller, spec).await {
        Ok(id) => Ok(json_response(
            StatusCode::CREATED,
            &json!({ "id": id.to_string() }),
        )),
        Err(Error::Conflict(Conflict::Exists)) => {
            let existing = app
                .engine
                .list_caches(&auth.caller)
                .await?
                .into_iter()
                .find(|cache| cache.display_name == body.display_name)
                .ok_or(Error::Conflict(Conflict::Exists))?;
            let mut stored = app.engine.get_owner_keys(&auth.caller, existing.id).await?;
            stored.sort_unstable();
            let same = stored == keys
                && body
                    .retention_days
                    .is_none_or(|days| days == existing.retention_days)
                && body
                    .hard_bytes
                    .is_none_or(|bytes| bytes == existing.hard_bytes);
            if same {
                Ok(json_response(
                    StatusCode::OK,
                    &json!({ "id": existing.id.to_string() }),
                ))
            } else {
                Err(Error::Conflict(Conflict::Exists).into())
            }
        }
        Err(error) => Err(error.into()),
    }
}

/// Deletes a cache: 202 once it is tombstoned, its removal follows asynchronously.
pub(crate) async fn delete<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    app.engine.delete_cache(&auth.caller, cache).await?;
    Ok(empty(StatusCode::ACCEPTED))
}

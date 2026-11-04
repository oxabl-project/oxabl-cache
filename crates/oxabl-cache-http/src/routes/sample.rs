//! `GET /v1/caches/{cache}/sample`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::json_response;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
pub(crate) struct Sample {
    namespace: String,
    n: usize,
}

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    Query(query): Query<Sample>,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    if query.namespace != "trusted" {
        return Err(ApiError::bad_request("namespace must be trusted"));
    }
    let ids = app.engine.sample(&auth.caller, cache, query.n).await?;
    auth.ctx.count("ids", ids.len() as u64);
    let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    Ok(json_response(StatusCode::OK, &json!({ "ids": ids })))
}

//! `POST` and `GET /v1/caches/{cache}/telemetry`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::body::read_limited;
use crate::error::ApiError;
use crate::json::json_response;
use crate::routes::versioned::require_binary;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use oxabl_cache_core::limits::TELEMETRY_PAGE;
use oxabl_cache_format::limits::TELEMETRY_BYTES;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    require_binary(&headers)?;
    let bytes = read_limited(body, TELEMETRY_BYTES).await?;
    let seq = app
        .engine
        .submit_telemetry(&auth.caller, cache, bytes.to_vec())
        .await?;
    app.metrics
        .inc("oxabl_cache_telemetry_reports_total", &[], 1);
    Ok(json_response(StatusCode::CREATED, &json!({ "seq": seq })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Page {
    #[serde(default)]
    after: u64,
    limit: Option<usize>,
}

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    Query(page): Query<Page>,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let rows = app
        .engine
        .telemetry(
            &auth.caller,
            cache,
            page.after,
            page.limit.unwrap_or(TELEMETRY_PAGE),
        )
        .await?;
    auth.ctx.count("reports", rows.len() as u64);
    let next = rows.last().map_or(page.after, |row| row.seq);
    let reports: Vec<_> = rows
        .iter()
        .map(|row| {
            json!({
                "seq": row.seq,
                "received_at": row.received_at,
                "body": base64::engine::general_purpose::STANDARD.encode(&row.body),
            })
        })
        .collect();
    Ok(json_response(
        StatusCode::OK,
        &json!({ "reports": reports, "next": next }),
    ))
}

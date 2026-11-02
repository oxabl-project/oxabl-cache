//! `POST /v1/caches/{cache}/reports`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{empty, read_json};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::usecases::ReportOutcome;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportBody {
    pack_id: String,
    segment_id: String,
    kind: String,
}

pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let body: ReportBody = read_json(body).await?;
    if body.kind != "hash_mismatch" {
        return Err(ApiError::bad_request("kind is hash_mismatch"));
    }
    let pack = body
        .pack_id
        .parse()
        .map_err(|_| ApiError::bad_request("pack_id is 32 lowercase hex characters"))?;
    let segment = body
        .segment_id
        .parse()
        .map_err(|_| ApiError::bad_request("segment_id is 64 lowercase hex characters"))?;
    let outcome = app
        .engine
        .report_corruption(&auth.caller, cache, pack, segment)
        .await?;
    let label = match outcome {
        ReportOutcome::Confirmed { entries_removed } => {
            auth.ctx.count("entries_removed", entries_removed);
            "confirmed"
        }
        ReportOutcome::Unconfirmed => "unconfirmed",
    };
    app.metrics
        .inc("oxabl_cache_reports_total", &[("outcome", label)], 1);
    Ok(empty(StatusCode::ACCEPTED))
}

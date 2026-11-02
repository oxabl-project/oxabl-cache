//! `POST /admin/v1/caches/{c}/scrub`.

use crate::app::{App, Backend};
use crate::auth::Operator;
use crate::error::ApiError;
use crate::json::json_response;
use crate::path::cache_id;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::json;
use std::sync::Arc;

pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    operator: Operator,
    Path(cache): Path<String>,
) -> Result<Response, ApiError> {
    let cache = cache_id(&cache)?;
    operator.ctx.set_cache(cache.to_string());
    let outcome = app.engine.scrub_cache(cache).await?;
    Ok(json_response(
        StatusCode::OK,
        &json!({
            "corrupt_segments": outcome.corrupt_segments,
            "entries_removed": outcome.entries_removed,
        }),
    ))
}

//! `GET /admin/v1/health`.

use crate::app::{App, Backend};
use crate::auth::Operator;
use crate::error::ApiError;
use crate::health::unreachable;
use crate::json::json_response;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::Error;
use serde_json::json;
use std::sync::Arc;

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
) -> Result<Response, ApiError> {
    let down = unreachable(&app).await;
    if !down.is_empty() {
        let mut error = ApiError::from(Error::unavailable(down.join(", ")));
        error.message = format!("unavailable: {}", down.join(", "));
        return Err(error);
    }
    Ok(json_response(
        StatusCode::OK,
        &json!({
            "metadata": "ok",
            "store": "ok",
            "gc_leader": app.gc_leader(),
        }),
    ))
}

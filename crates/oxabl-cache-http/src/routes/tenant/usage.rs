//! `GET /v1/tenant/usage?month=YYYY-MM`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::json_response;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
pub(crate) struct Month {
    month: String,
}

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Query(query): Query<Month>,
) -> Result<Response, ApiError> {
    let _ = app.engine.flush_usage().await;
    let totals = app.engine.tenant_usage(&auth.caller, &query.month).await?;
    let usage: Vec<Value> = totals
        .iter()
        .map(|total| {
            json!({
                "cache": total.cache.to_string(),
                "metric": total.metric.name(),
                "value": total.value,
            })
        })
        .collect();
    Ok(json_response(
        StatusCode::OK,
        &json!({ "month": query.month, "usage": usage }),
    ))
}

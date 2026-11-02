//! `GET /admin/v1/usage?month=2026-10&format=csv`.

use crate::app::{App, Backend};
use crate::auth::Operator;
use crate::error::ApiError;
use crate::json::json_response;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt::Write;
use std::sync::Arc;

#[derive(Deserialize)]
pub(crate) struct UsageQuery {
    month: String,
    format: Option<String>,
}

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
    Query(query): Query<UsageQuery>,
) -> Result<Response, ApiError> {
    let _ = app.engine.flush_usage().await;
    let totals = app.engine.usage_export(&query.month).await?;
    match query.format.as_deref() {
        None | Some("json") => {
            let rows: Vec<Value> = totals
                .iter()
                .map(|total| {
                    json!({
                        "tenant": total.tenant.to_string(),
                        "cache": total.cache.to_string(),
                        "metric": total.metric.name(),
                        "value": total.value,
                    })
                })
                .collect();
            Ok(json_response(
                StatusCode::OK,
                &json!({ "month": query.month, "usage": rows }),
            ))
        }
        Some("csv") => {
            let mut csv = String::from("month,tenant,cache,metric,value\n");
            for total in &totals {
                let _ = writeln!(
                    csv,
                    "{},{},{},{},{}",
                    query.month,
                    total.tenant,
                    total.cache,
                    total.metric.name(),
                    total.value
                );
            }
            let mut response = Response::new(Body::from(csv));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv"));
            Ok(response)
        }
        Some(_) => Err(ApiError::bad_request("format is json or csv")),
    }
}

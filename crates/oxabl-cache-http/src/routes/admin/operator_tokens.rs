//! `GET|POST /admin/v1/operator-tokens`, `DELETE /admin/v1/operator-tokens/{id}`.

use crate::app::{App, Backend};
use crate::auth::Operator;
use crate::error::ApiError;
use crate::json::{empty, json_response, read_json};
use crate::path::credential_id;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::time::DAY_SECS;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewToken {
    name: String,
    expires_in_days: Option<i64>,
}

pub(crate) async fn list<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
) -> Result<Response, ApiError> {
    let tokens = app.engine.list_operator_tokens().await?;
    let tokens: Vec<Value> = tokens
        .iter()
        .map(|token| {
            json!({
                "id": token.id.to_string(),
                "name": token.name,
                "created_at": token.created_at,
                "expires_at": token.expires_at,
                "revoked_at": token.revoked_at,
            })
        })
        .collect();
    Ok(json_response(
        StatusCode::OK,
        &json!({ "operator_tokens": tokens }),
    ))
}

pub(crate) async fn create<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
    body: Body,
) -> Result<Response, ApiError> {
    let body: NewToken = read_json(body).await?;
    let created = app
        .engine
        .create_operator_token(
            body.name,
            body.expires_in_days
                .map(|days| days.saturating_mul(DAY_SECS)),
        )
        .await?;
    Ok(json_response(
        StatusCode::CREATED,
        &json!({
            "id": created.id.to_string(),
            "token": created.secret,
            "expires_at": created.expires_at,
        }),
    ))
}

pub(crate) async fn revoke<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    app.engine
        .revoke_operator_token(credential_id(&id)?)
        .await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

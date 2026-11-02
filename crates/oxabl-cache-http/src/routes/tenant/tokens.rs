//! `GET|POST /v1/tenant/tokens`, `DELETE /v1/tenant/tokens/{id}`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{empty, json_response, parse_scopes, read_json, scope_names};
use crate::path::{cache_id, credential_id, principal_id};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::domain::scope::Grant;
use oxabl_cache_core::time::DAY_SECS;
use oxabl_cache_core::usecases::TokenSpec;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewToken {
    name: String,
    scopes: Vec<String>,
    #[serde(default)]
    caches: Vec<String>,
    principal: Option<String>,
    expires_in_days: Option<i64>,
}

pub(crate) fn grants_json(grants: &[Grant]) -> Value {
    let grants: Vec<Value> = grants
        .iter()
        .map(|grant| {
            json!({
                "cache": grant.cache.map(|cache| cache.to_string()),
                "scopes": scope_names(grant.scopes),
            })
        })
        .collect();
    json!(grants)
}

pub(crate) async fn list<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
) -> Result<Response, ApiError> {
    let tokens = app.engine.list_tokens(&auth.caller).await?;
    let tokens: Vec<Value> = tokens
        .iter()
        .map(|token| {
            json!({
                "id": token.id.to_string(),
                "name": token.name,
                "principal": token.principal.to_string(),
                "created_at": token.created_at,
                "expires_at": token.expires_at,
                "revoked_at": token.revoked_at,
                "grants": grants_json(&token.grants),
            })
        })
        .collect();
    Ok(json_response(StatusCode::OK, &json!({ "tokens": tokens })))
}

pub(crate) async fn create<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    let body: NewToken = read_json(body).await?;
    let spec = TokenSpec {
        name: body.name,
        principal: body.principal.as_deref().map(principal_id).transpose()?,
        scopes: parse_scopes(&body.scopes)?,
        caches: body
            .caches
            .iter()
            .map(|text| cache_id(text))
            .collect::<Result<_, _>>()?,
        expires_in_secs: body
            .expires_in_days
            .map(|days| days.saturating_mul(DAY_SECS)),
    };
    let created = app.engine.create_token(&auth.caller, spec).await?;
    Ok(json_response(
        StatusCode::CREATED,
        &json!({
            "id": created.id.to_string(),
            "token": created.secret,
            "principal": created.principal.to_string(),
            "expires_at": created.expires_at,
        }),
    ))
}

pub(crate) async fn revoke<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    app.engine
        .revoke_token(&auth.caller, credential_id(&id)?)
        .await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

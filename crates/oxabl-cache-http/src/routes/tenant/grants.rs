//! `GET|PUT /v1/tenant/grants`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{empty, json_response, parse_scopes, read_json, scope_names};
use crate::path::{cache_id, principal_id};
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::domain::scope::Grant;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantBody {
    principal: String,
    cache: Option<String>,
    scopes: Vec<String>,
}

pub(crate) async fn list<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
) -> Result<Response, ApiError> {
    let grants = app.engine.list_grants(&auth.caller).await?;
    let grants: Vec<Value> = grants
        .iter()
        .map(|(principal, grant)| {
            json!({
                "principal": principal.to_string(),
                "cache": grant.cache.map(|cache| cache.to_string()),
                "scopes": scope_names(grant.scopes),
            })
        })
        .collect();
    Ok(json_response(StatusCode::OK, &json!({ "grants": grants })))
}

pub(crate) async fn put<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    let body: GrantBody = read_json(body).await?;
    let grant = Grant {
        cache: body.cache.as_deref().map(cache_id).transpose()?,
        scopes: parse_scopes(&body.scopes)?,
    };
    app.engine
        .set_grant(&auth.caller, principal_id(&body.principal)?, grant)
        .await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

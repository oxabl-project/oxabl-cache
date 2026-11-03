//! `…/v1/tenant/oidc/issuers` and `…/oidc/policies`: the issuer allowlist and the
//! workload trust policies.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{empty, json_response, parse_scopes, read_json, scope_names};
use crate::path::cache_id;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::oidc::{Issuer, IssuerShape, WorkloadPolicy};
use oxabl_cache_core::domain::scope::Grant;
use oxabl_cache_core::usecases::{IssuerSpec, PolicySpec};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewIssuer {
    issuer: String,
    shape: Option<String>,
    #[serde(default)]
    id_claims: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssuerName {
    issuer: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantBody {
    cache: String,
    scopes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPolicy {
    name: String,
    issuer: String,
    claims: Map<String, Value>,
    grants: Vec<GrantBody>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyName {
    name: String,
}

fn issuer_json(issuer: &Issuer) -> Value {
    json!({
        "issuer": issuer.url,
        "shape": issuer.shape.name(),
        "id_claims": issuer.id_claim_sets(),
        "created_at": issuer.created_at,
    })
}

fn policy_json(policy: &WorkloadPolicy) -> Value {
    let grants: Vec<Value> = policy
        .grants
        .iter()
        .map(|grant| {
            json!({
                "cache": grant.cache.map(|cache| cache.to_string()),
                "scopes": scope_names(grant.scopes),
            })
        })
        .collect();
    json!({
        "id": policy.id.to_string(),
        "name": policy.name,
        "issuer": policy.issuer,
        "claims": policy.conditions.iter().map(|c| c.claim.as_str()).collect::<Vec<_>>(),
        "grants": grants,
        "created_at": policy.created_at,
    })
}

pub(crate) async fn list_issuers<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
) -> Result<Response, ApiError> {
    let issuers = app.engine.list_issuers(&auth.caller).await?;
    let issuers: Vec<Value> = issuers.iter().map(issuer_json).collect();
    Ok(json_response(
        StatusCode::OK,
        &json!({ "issuers": issuers }),
    ))
}

/// Registers an issuer after the verifier vetted it. The identical registration again
/// is a 200, a different shape or id claims a 409.
pub(crate) async fn add_issuer<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    auth.caller.require_tenant_admin()?;
    let body: NewIssuer = read_json(body).await?;
    let shape = match body.shape.as_deref() {
        None => IssuerShape::Custom,
        Some(name) => IssuerShape::parse(name)
            .ok_or_else(|| ApiError::bad_request("shape is github, gitlab or custom"))?,
    };
    let spec = IssuerSpec {
        url: body.issuer.clone(),
        shape,
        id_claims: body.id_claims,
    };
    let created = app
        .engine
        .register_issuer(&auth.caller, &app.verifier, spec)
        .await?;
    let stored = app
        .engine
        .list_issuers(&auth.caller)
        .await?
        .into_iter()
        .find(|issuer| issuer.url == body.issuer)
        .ok_or(Error::NotFound)?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok(json_response(status, &issuer_json(&stored)))
}

pub(crate) async fn delete_issuer<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    auth.caller.require_tenant_admin()?;
    let body: IssuerName = read_json(body).await?;
    app.engine.remove_issuer(&auth.caller, body.issuer).await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

pub(crate) async fn list_policies<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
) -> Result<Response, ApiError> {
    let policies = app.engine.list_policies(&auth.caller).await?;
    let policies: Vec<Value> = policies.iter().map(policy_json).collect();
    Ok(json_response(
        StatusCode::OK,
        &json!({ "policies": policies }),
    ))
}

fn claim_text(value: &Value) -> Result<String, ApiError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) if number.is_i64() || number.is_u64() => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        _ => Err(ApiError::bad_request(
            "a claim value is a string, a whole number or a boolean",
        )),
    }
}

/// Creates a workload policy. The name is the natural key: the identical policy again
/// is a 200, a different one of the same name a 409.
pub(crate) async fn add_policy<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    auth.caller.require_tenant_admin()?;
    let body: NewPolicy = read_json(body).await?;
    let claims = body
        .claims
        .iter()
        .map(|(name, value)| Ok((name.clone(), claim_text(value)?)))
        .collect::<Result<Vec<_>, ApiError>>()?;
    let grants = body
        .grants
        .iter()
        .map(|grant| {
            Ok(Grant {
                cache: Some(cache_id(&grant.cache)?),
                scopes: parse_scopes(&grant.scopes)?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let (id, created) = app
        .engine
        .create_policy(
            &auth.caller,
            PolicySpec {
                name: body.name,
                issuer: body.issuer,
                claims,
                grants,
            },
        )
        .await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok(json_response(status, &json!({ "id": id.to_string() })))
}

pub(crate) async fn delete_policy<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    auth.caller.require_tenant_admin()?;
    let body: PolicyName = read_json(body).await?;
    app.engine.remove_policy(&auth.caller, body.name).await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

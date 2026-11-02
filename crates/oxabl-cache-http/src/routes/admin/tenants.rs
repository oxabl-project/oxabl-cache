//! `GET|POST /admin/v1/tenants`, `PATCH|DELETE /admin/v1/tenants/{t}`,
//! `PUT /admin/v1/tenants/{t}/quota`, `POST /admin/v1/tenants/{t}/tokens`.

use crate::app::{App, Backend};
use crate::auth::Operator;
use crate::error::ApiError;
use crate::json::{json_response, read_json};
use crate::path::tenant_id;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::domain::tenant::{Tenant, TenantLimits, TenantStatus};
use oxabl_cache_core::time::DAY_SECS;
use oxabl_cache_core::usecases::TenantSpec;
use oxabl_cache_core::{Conflict, Error};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewTenant {
    name: String,
    hard_bytes: Option<u64>,
    egress_bytes: Option<u64>,
    rate: Option<u32>,
    burst: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Quota {
    hard_bytes: Option<u64>,
    egress_bytes: Option<u64>,
    rate: u32,
    burst: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    status: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminToken {
    name: String,
    expires_in_days: Option<i64>,
}

fn tenant_json(tenant: &Tenant) -> Value {
    json!({
        "id": tenant.id.to_string(),
        "name": tenant.name,
        "status": match tenant.status {
            TenantStatus::Active => "active",
            TenantStatus::Suspended => "suspended",
        },
        "hard_bytes": tenant.hard_bytes,
        "egress_bytes": tenant.egress_bytes,
        "rate": tenant.rate,
        "burst": tenant.burst,
        "created_at": tenant.created_at,
    })
}

pub(crate) async fn list<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
) -> Result<Response, ApiError> {
    let tenants = app.engine.list_tenants().await?;
    let tenants: Vec<Value> = tenants.iter().map(tenant_json).collect();
    Ok(json_response(
        StatusCode::OK,
        &json!({ "tenants": tenants }),
    ))
}

/// Creates a tenant. The name is the natural key: the identical request again is a 200.
pub(crate) async fn create<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
    body: Body,
) -> Result<Response, ApiError> {
    let body: NewTenant = read_json(body).await?;
    let spec = TenantSpec {
        name: body.name.clone(),
        hard_bytes: body.hard_bytes,
        egress_bytes: body.egress_bytes,
        rate: body.rate.unwrap_or(app.defaults.rate),
        burst: body.burst.unwrap_or(app.defaults.burst),
    };
    match app.engine.create_tenant(spec.clone()).await {
        Ok(id) => {
            let created = app.engine.get_tenant(id).await?;
            Ok(json_response(StatusCode::CREATED, &tenant_json(&created)))
        }
        Err(Error::Conflict(Conflict::Exists)) => {
            let existing = app
                .engine
                .list_tenants()
                .await?
                .into_iter()
                .find(|tenant| tenant.name == body.name)
                .ok_or(Error::Conflict(Conflict::Exists))?;
            let same = existing.hard_bytes == spec.hard_bytes
                && existing.egress_bytes == spec.egress_bytes
                && existing.rate == spec.rate
                && existing.burst == spec.burst;
            if same {
                Ok(json_response(StatusCode::OK, &tenant_json(&existing)))
            } else {
                Err(Error::Conflict(Conflict::Exists).into())
            }
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn patch<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
    Path(tenant): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let tenant = tenant_id(&tenant)?;
    let body: Patch = read_json(body).await?;
    let status = match body.status.as_str() {
        "active" => TenantStatus::Active,
        "suspended" => TenantStatus::Suspended,
        _ => return Err(ApiError::bad_request("status is active or suspended")),
    };
    app.engine.set_tenant_status(tenant, status).await?;
    let found = app.engine.get_tenant(tenant).await?;
    Ok(json_response(StatusCode::OK, &tenant_json(&found)))
}

/// Deleting a tenant needs the asynchronous teardown of its caches, which this build lacks.
pub(crate) async fn delete<B: Backend>(
    State(_app): State<Arc<App<B>>>,
    _operator: Operator,
    Path(tenant): Path<String>,
) -> Result<Response, ApiError> {
    tenant_id(&tenant)?;
    Err(ApiError::not_implemented(
        "deleting a tenant is not implemented in this build",
    ))
}

pub(crate) async fn quota<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
    Path(tenant): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let tenant = tenant_id(&tenant)?;
    let body: Quota = read_json(body).await?;
    app.engine
        .set_tenant_limits(
            tenant,
            TenantLimits {
                hard_bytes: body.hard_bytes,
                egress_bytes: body.egress_bytes,
                rate: body.rate,
                burst: body.burst,
            },
        )
        .await?;
    let found = app.engine.get_tenant(tenant).await?;
    Ok(json_response(StatusCode::OK, &tenant_json(&found)))
}

pub(crate) async fn token<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
    Path(tenant): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let tenant = tenant_id(&tenant)?;
    let body: AdminToken = read_json(body).await?;
    let created = app
        .engine
        .create_tenant_admin_token(
            tenant,
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

//! `POST /admin/v1/tenants/{t}/admins`: the first human administrator of a tenant.

use crate::app::{App, Backend};
use crate::auth::Operator;
use crate::error::ApiError;
use crate::json::{json_response, read_json};
use crate::path::tenant_id;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admin {
    issuer: String,
    subject: String,
}

/// Binds a `tenant:admin` user to an issuer and subject: 201 when new, 200 when the
/// same subject already is the tenant's administrator.
pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
    Path(tenant): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let tenant = tenant_id(&tenant)?;
    let body: Admin = read_json(body).await?;
    let (id, created) = app
        .engine
        .bootstrap_tenant_admin(&app.verifier, tenant, body.issuer, body.subject)
        .await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok(json_response(status, &json!({ "id": id.to_string() })))
}

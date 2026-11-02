//! `…/v1/tenant/oidc/issuers` and `…/oidc/policies`: the issuer allowlist and the
//! workload trust policies belong to the OIDC adapter, which this build lacks.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use axum::extract::State;
use axum::response::Response;
use std::sync::Arc;

pub(crate) async fn not_configured<B: Backend>(
    State(_app): State<Arc<App<B>>>,
    auth: Authed,
) -> Result<Response, ApiError> {
    auth.caller.require_tenant_admin()?;
    Err(ApiError::not_implemented(
        "OIDC trust is not configured on this server",
    ))
}

//! `POST /v1/auth/oidc` and `DELETE /v1/auth/session`.

use crate::app::{App, Backend};
use crate::auth::bearer;
use crate::error::ApiError;
use crate::json::{empty, read_json};
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use oxabl_cache_core::Error;
use oxabl_cache_core::ports::IdentityVerifier;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OidcBody {
    tenant: String,
    id_token: String,
}

/// Exchanges an identity token for a session. A verified identity still needs a
/// matching trust policy; this build registers none, so none matches.
pub(crate) async fn oidc<B: Backend>(
    State(app): State<Arc<App<B>>>,
    body: Body,
) -> Result<Response, ApiError> {
    let body: OidcBody = read_json(body).await?;
    let tenant = body
        .tenant
        .parse()
        .map_err(|_| ApiError::bad_request("tenant is t_ and 26 base32 characters"))?;
    let count = |result: &str| {
        app.metrics
            .inc("oxabl_cache_oidc_exchanges_total", &[("result", result)], 1);
    };
    match app.verifier.verify(&tenant, &body.id_token).await {
        Err(Error::Unavailable(_)) => {
            count("not_configured");
            Err(ApiError::not_implemented(
                "OIDC is not configured on this server",
            ))
        }
        Err(_) | Ok(_) => {
            count("rejected");
            Err(Error::Unauthenticated.into())
        }
    }
}

pub(crate) async fn revoke<B: Backend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    app.engine.revoke_session(bearer(&headers)?).await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

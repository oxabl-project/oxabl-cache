//! `POST /v1/auth/oidc` and `DELETE /v1/auth/session`.

use crate::app::{App, Backend};
use crate::auth::bearer;
use crate::error::ApiError;
use crate::json::{empty, json_response, read_json};
use crate::routes::tenant::tokens::grants_json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use oxabl_cache_core::Error;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OidcBody {
    tenant: String,
    id_token: String,
}

/// Exchanges an identity token for a session. Every refusal is the same 401, so a
/// caller learns nothing about which check failed.
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
    match app
        .engine
        .exchange_identity(&app.verifier, tenant, &body.id_token)
        .await
    {
        Ok(exchanged) => {
            count("accepted");
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "token": exchanged.session.secret,
                    "expires_at": exchanged.session.expires_at,
                    "grants": grants_json(&exchanged.grants),
                }),
            ))
        }
        Err(Error::Unavailable(detail)) => {
            count("unavailable");
            Err(Error::Unavailable(detail).into())
        }
        Err(Error::TenantSuspended) => {
            count("rejected");
            Err(Error::TenantSuspended.into())
        }
        Err(_) => {
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

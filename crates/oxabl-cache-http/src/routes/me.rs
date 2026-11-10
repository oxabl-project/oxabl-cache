//! `GET /v1/me`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{json_response, scope_names};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::json;
use std::sync::Arc;

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
) -> Result<Response, ApiError> {
    let me = app.engine.me(&auth.caller).await?;
    let caches: Vec<_> = me
        .caches
        .iter()
        .map(|cache| {
            json!({
                "id": cache.id.to_string(),
                "display_name": cache.display_name,
                "scopes": scope_names(cache.scopes),
                "epoch": cache.epoch,
                "roster_version": cache.roster_version,
                "quota": {
                    "hard_bytes": cache.quota.hard_bytes,
                    "live_bytes": cache.quota.live_bytes,
                    "pending_bytes": cache.quota.pending_bytes,
                    "stored_bytes": cache.quota.stored_bytes,
                    "reclaimable_bytes": cache.quota.reclaimable_bytes,
                },
            })
        })
        .collect();
    Ok(json_response(
        StatusCode::OK,
        &json!({
            "tenant": me.tenant.to_string(),
            "principal": me.principal.to_string(),
            "tenant_admin": me.tenant_admin,
            "caches": caches,
        }),
    ))
}

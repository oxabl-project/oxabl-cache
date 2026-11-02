//! `POST /v1/caches/{cache}/purge`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{json_response, read_json};
use crate::path::principal_id;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::usecases::PurgeScope;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PurgeBody {
    below_epoch: Option<u32>,
    namespace: Option<String>,
    user: Option<String>,
}

pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let body: PurgeBody = read_json(body).await?;
    let scope = match (
        body.below_epoch,
        body.namespace.as_deref(),
        body.user.as_deref(),
    ) {
        (Some(epoch), None, None) => PurgeScope::BelowEpoch(epoch),
        (None, Some("personal"), Some(user)) => PurgeScope::PersonalOf(if user == "me" {
            auth.caller.principal
        } else {
            principal_id(user)?
        }),
        _ => {
            return Err(ApiError::bad_request(
                "purge takes below_epoch, or namespace personal with a user",
            ));
        }
    };
    let marked = app.engine.purge(&auth.caller, cache, scope).await?;
    Ok(json_response(
        StatusCode::ACCEPTED,
        &json!({ "entries_marked": marked }),
    ))
}

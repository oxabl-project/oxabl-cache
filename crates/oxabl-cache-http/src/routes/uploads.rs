//! `POST /v1/caches/{cache}/uploads`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{json_response, read_json};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use oxabl_cache_core::domain::namespace::NamespaceKind;
use oxabl_cache_core::usecases::{PackRequest, UploadOutcome};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackBody {
    client_ref: String,
    size: u64,
    sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadBody {
    namespace: String,
    packs: Vec<PackBody>,
}

fn pack_request(body: PackBody) -> Result<PackRequest, ApiError> {
    let client_ref = body
        .client_ref
        .parse()
        .map_err(|_| ApiError::bad_request("client_ref is 32 lowercase hex characters"))?;
    let sha256 = body
        .sha256
        .map(|text| {
            STANDARD
                .decode(text)
                .ok()
                .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
                .ok_or_else(|| ApiError::bad_request("sha256 is the base64 of 32 bytes"))
        })
        .transpose()?;
    Ok(PackRequest {
        client_ref,
        size: body.size,
        sha256,
    })
}

fn outcome_json(outcome: UploadOutcome) -> Value {
    match outcome {
        UploadOutcome::Reserved { pack, url } => {
            let headers: Map<String, Value> = url
                .headers
                .into_iter()
                .map(|(name, value)| (name, Value::from(value)))
                .collect();
            json!({
                "status": "ok",
                "pack_id": pack.to_string(),
                "url": url.url,
                "method": url.method,
                "headers": headers,
                "expires_at": url.expires_at,
            })
        }
        UploadOutcome::Committed { pack } => {
            json!({"status": "committed", "pack_id": pack.to_string()})
        }
        UploadOutcome::QuotaExceeded => json!({"status": "quota_exceeded"}),
    }
}

pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let body: UploadBody = read_json(body).await?;
    let kind = match body.namespace.as_str() {
        "trusted" => NamespaceKind::Trusted,
        "personal" => NamespaceKind::Personal,
        _ => return Err(ApiError::bad_request("namespace is trusted or personal")),
    };
    let packs = body
        .packs
        .into_iter()
        .map(pack_request)
        .collect::<Result<Vec<_>, _>>()?;
    auth.ctx.count("packs", packs.len() as u64);
    let outcomes = app
        .engine
        .reserve_uploads(&auth.caller, cache, kind, packs)
        .await?;
    let packs: Vec<Value> = outcomes.into_iter().map(outcome_json).collect();
    Ok(json_response(StatusCode::OK, &json!({ "packs": packs })))
}

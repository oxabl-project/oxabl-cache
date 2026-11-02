//! `POST /v1/caches/{cache}/urls`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{json_response, read_json};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UrlsBody {
    packs: Vec<String>,
}

pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    let body: UrlsBody = read_json(body).await?;
    let packs = body
        .packs
        .iter()
        .map(|text| {
            text.parse()
                .map_err(|_| ApiError::bad_request("pack ids are 32 lowercase hex characters"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let issued = app.engine.refresh_urls(&auth.caller, cache, packs).await?;
    let urls: Map<String, Value> = issued
        .into_iter()
        .map(|(pack, url)| {
            (
                pack.to_string(),
                json!({"url": url.url, "expires_at": url.expires_at}),
            )
        })
        .collect();
    Ok(json_response(StatusCode::OK, &json!({ "urls": urls })))
}

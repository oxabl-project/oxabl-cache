//! `POST /v1/caches/{cache}/lookup`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::body::read_limited;
use crate::error::ApiError;
use crate::frames::{distinct_hits, encode_lookup, parse_lookup};
use crate::routes::versioned::{binary, require_binary};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use std::sync::Arc;

const LOOKUP_BODY_BYTES: usize = 12 + 32 * 10_000;

pub(crate) async fn post<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(cache): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let cache = auth.cache(&cache)?;
    require_binary(&headers)?;
    let bytes = read_limited(body, LOOKUP_BODY_BYTES).await?;
    let parsed = parse_lookup(cache, &bytes)?;
    let probed = parsed.request.ids.len();
    let answer = app.engine.lookup(&auth.caller, parsed.request).await?;
    let hits = distinct_hits(&answer);
    app.metrics.inc(
        "oxabl_cache_lookup_ids_total",
        &[("result", "hit")],
        hits as u64,
    );
    app.metrics.inc(
        "oxabl_cache_lookup_ids_total",
        &[("result", "miss")],
        probed.saturating_sub(hits) as u64,
    );
    auth.ctx.count("ids", probed as u64);
    auth.ctx.count("hits", hits as u64);
    let encoded = encode_lookup(&parsed.shape, &answer)?;
    auth.ctx.count("bytes_out", encoded.len() as u64);
    Ok(binary(encoded, None))
}

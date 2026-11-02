//! `PUT|GET /v1/caches/{cache}/objects/{pack_id}`: the local-disk object route,
//! authorised by an HMAC-signed URL instead of a bearer token.

use crate::app::{App, Backend};
use crate::error::ApiError;
use crate::json::empty;
use crate::path::{cache_id, pack_id};
use crate::routes::range::requested_span;
use crate::routes::served::span_stream;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use bytes::Bytes;
use http_body_util::BodyExt;
use oxabl_cache_core::Error;
use oxabl_cache_core::ports::{ObjectHead, ObjectKey, ObjectStore};
use oxabl_cache_store_fs::{FsStore, UrlRequest};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
pub(crate) struct Signature {
    exp: i64,
    sig: String,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

fn verify(
    local: &FsStore,
    method: &str,
    key: &ObjectKey,
    signature: &Signature,
    length: Option<u64>,
) -> Result<(), ApiError> {
    local
        .verify_url(
            &UrlRequest {
                method,
                cache: key.cache,
                pack: key.pack,
                expires_at: signature.exp,
                content_length: length,
                signature: &signature.sig,
            },
            now(),
        )
        .map_err(|_| ApiError::forbidden("the URL is invalid or expired"))
}

enum Chunk {
    Data(Bytes),
    End,
}

async fn write_streamed(
    local: &FsStore,
    key: &ObjectKey,
    body: Body,
    size: u64,
) -> Result<ObjectHead, ApiError> {
    let mut writer = local.writer(key)?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Chunk>(4);
    let worker = tokio::task::spawn_blocking(move || {
        while let Some(chunk) = receiver.blocking_recv() {
            match chunk {
                Chunk::Data(bytes) => writer.write_all(&bytes)?,
                Chunk::End => return writer.finish(Some(size)),
            }
        }
        Err(Error::bad_request("the upload was interrupted"))
    });
    let mut body = body;
    let mut received = 0u64;
    while let Some(frame) = body.frame().await {
        let frame =
            frame.map_err(|_| ApiError::bad_request("the request body could not be read"))?;
        if let Ok(data) = frame.into_data() {
            received += data.len() as u64;
            if received > size {
                return Err(ApiError::too_large());
            }
            if sender.send(Chunk::Data(data)).await.is_err() {
                break;
            }
        }
    }
    let _ = sender.send(Chunk::End).await;
    drop(sender);
    let finished = worker
        .await
        .map_err(|error| Error::internal(format!("upload task: {error}")))?;
    Ok(finished?)
}

pub(crate) async fn put<B: Backend>(
    State(app): State<Arc<App<B>>>,
    Path((cache, pack)): Path<(String, String)>,
    Query(signature): Query<Signature>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let local = app.local.as_ref().ok_or_else(ApiError::not_found)?;
    let key = ObjectKey {
        cache: cache_id(&cache)?,
        pack: pack_id(&pack)?,
    };
    let length = content_length(&headers)
        .ok_or_else(|| ApiError::bad_request("content-length is required"))?;
    verify(local, "PUT", &key, &signature, Some(length))?;
    app.engine
        .pack_accepts_put(key.cache, key.pack, length)
        .await?;
    let head = write_streamed(local, &key, body, length).await?;
    let mut response = empty(StatusCode::OK);
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{}\"", head.etag)) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    Ok(response)
}

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    Path((cache, pack)): Path<(String, String)>,
    Query(signature): Query<Signature>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let local = app.local.as_ref().ok_or_else(ApiError::not_found)?;
    let key = ObjectKey {
        cache: cache_id(&cache)?,
        pack: pack_id(&pack)?,
    };
    verify(local, "GET", &key, &signature, None)?;
    app.engine.pack_serves_get(key.cache, key.pack).await?;
    let head = app
        .engine
        .object_store()
        .head(&key)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let (start, end, partial) = requested_span(&headers, head.size)?;
    let length = end - start + 1;
    let stream = span_stream(Arc::clone(&app), key, head.etag.clone(), start, end);
    let mut response = Response::new(Body::from_stream(stream));
    let out = response.headers_mut();
    out.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{}\"", head.etag)) {
        out.insert(header::ETAG, etag);
    }
    if partial {
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
        if let Ok(range) = HeaderValue::from_str(&format!("bytes {start}-{end}/{}", head.size)) {
            response.headers_mut().insert(header::CONTENT_RANGE, range);
        }
    }
    Ok(response)
}

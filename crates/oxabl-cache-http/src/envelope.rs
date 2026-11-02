//! The outermost layer: request ids, the error envelope, metrics and the access log.

use crate::app::{App, Backend};
use crate::ctx::RequestCtx;
use crate::error::ApiError;
use crate::log::{Level, emit};
use axum::body::Body;
use axum::extract::{MatchedPath, Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use oxabl_cache_format::RequestId;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

const REQUEST_ID: &str = "x-request-id";

fn request_id(request: &Request) -> String {
    let supplied = request
        .headers()
        .get(REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .filter(|text| RequestId::from_text(text).is_ok());
    match supplied {
        Some(text) => text.to_owned(),
        None => {
            let mut bytes = [0u8; RequestId::LEN];
            getrandom::fill(&mut bytes).expect("the operating system provides randomness");
            RequestId(bytes).to_text()
        }
    }
}

/// Records the matched route template for the log and the metrics.
pub(crate) async fn label_route(request: Request, next: Next) -> Response {
    if let (Some(ctx), Some(path)) = (
        request.extensions().get::<RequestCtx>(),
        request.extensions().get::<MatchedPath>(),
    ) {
        ctx.set_route(path.as_str());
    }
    next.run(request).await
}

fn render_error(error: &ApiError, request_id: &str) -> Value {
    let mut body = json!({
        "code": error.code,
        "message": error.message,
        "retryable": error.retryable,
        "request_id": request_id,
    });
    if let (Some(reason), Some(fields)) = (error.reason, body.as_object_mut()) {
        fields.insert("reason".into(), Value::from(reason));
    }
    json!({ "error": body })
}

/// Wraps every request of a listener.
pub(crate) async fn envelope<B: Backend>(
    State(app): State<Arc<App<B>>>,
    mut request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let ctx = RequestCtx::new(request_id(&request));
    let method = request.method().clone();
    request.extensions_mut().insert(ctx.clone());
    let mut response = next.run(request).await;

    let status = response.status();
    if status.is_client_error() || status.is_server_error() {
        let error = response
            .extensions()
            .get::<ApiError>()
            .cloned()
            .unwrap_or_else(|| ApiError::for_status(status));
        if let Some(detail) = &error.detail {
            emit(
                Level::Error,
                "request_failed",
                vec![
                    ("request_id", Value::from(ctx.id.as_str())),
                    ("route", Value::from(ctx.route())),
                    ("detail", Value::from(detail.as_str())),
                ],
            );
        }
        let body = render_error(&error, &ctx.id).to_string();
        let headers = response.headers_mut();
        headers.remove(header::CONTENT_LENGTH);
        headers.remove(header::CONTENT_TYPE);
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        if status == axum::http::StatusCode::UNAUTHORIZED {
            headers.insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        *response.body_mut() = Body::from(body);
    }
    if let Ok(value) = HeaderValue::from_str(&ctx.id) {
        response.headers_mut().insert(REQUEST_ID, value);
    }
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );

    let elapsed = started.elapsed();
    let route = ctx.route();
    let status_text = status.as_u16().to_string();
    let labels = [("route", route.as_str()), ("status", status_text.as_str())];
    app.metrics
        .inc("oxabl_cache_http_requests_total", &labels, 1);
    app.metrics.observe(
        "oxabl_cache_http_request_seconds",
        &labels,
        elapsed.as_secs_f64(),
    );
    let level = if status.is_server_error() {
        Level::Error
    } else {
        Level::Info
    };
    let mut fields = vec![
        ("request_id", Value::from(ctx.id.as_str())),
        ("method", Value::from(method.as_str())),
        ("route", Value::from(route.as_str())),
        ("status", Value::from(status.as_u16())),
        (
            "duration_ms",
            Value::from(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)),
        ),
    ];
    fields.extend(ctx.log_fields());
    emit(level, "request", fields);
    response
}

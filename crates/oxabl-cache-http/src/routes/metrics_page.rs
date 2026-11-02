//! `GET /metrics` on the metrics listener.

use crate::app::{App, Backend};
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::Response;
use std::sync::Arc;

pub(crate) async fn get<B: Backend>(State(app): State<Arc<App<B>>>) -> Response {
    let mut response = Response::new(Body::from(app.metrics.render()));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    response
}

//! JSON request and response helpers.

use crate::body::read_limited;
use crate::error::ApiError;
use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use oxabl_cache_core::domain::scope::{Scope, ScopeSet};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// The size limit of JSON bodies.
pub(crate) const JSON_BODY_BYTES: usize = 1024 * 1024;

/// Parses a JSON body; an unknown field is a 400.
pub(crate) async fn read_json<T: DeserializeOwned>(body: Body) -> Result<T, ApiError> {
    let bytes = read_limited(body, JSON_BODY_BYTES).await?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ApiError::bad_request(format!("malformed JSON body: {error}")))
}

/// A JSON response.
pub(crate) fn json_response(status: StatusCode, value: &Value) -> Response {
    let mut response = Response::new(Body::from(value.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

/// An empty response of the given status.
pub(crate) fn empty(status: StatusCode) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

/// The protocol names of a scope set.
pub(crate) fn scope_names(scopes: ScopeSet) -> Value {
    let names: Vec<&str> = [
        Scope::Read,
        Scope::WritePersonal,
        Scope::WriteTrusted,
        Scope::CacheAdmin,
        Scope::TenantAdmin,
    ]
    .into_iter()
    .filter(|scope| scopes.contains(*scope))
    .map(Scope::name)
    .collect();
    json!(names)
}

/// Parses scope names; an unknown name is a 400.
pub(crate) fn parse_scopes(names: &[String]) -> Result<ScopeSet, ApiError> {
    names
        .iter()
        .map(|name| {
            Scope::parse(name).ok_or_else(|| ApiError::bad_request(format!("unknown scope {name}")))
        })
        .collect()
}

//! `GET /admin/v1/health`.

use crate::app::{App, Backend};
use crate::auth::Operator;
use crate::error::ApiError;
use crate::json::json_response;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::Error;
use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::{ObjectKey, ObjectStore};
use serde_json::json;
use std::sync::Arc;

pub(crate) async fn get<B: Backend>(
    State(app): State<Arc<App<B>>>,
    _operator: Operator,
) -> Result<Response, ApiError> {
    let metadata = app.engine.live_node_count().await.is_ok();
    let probe = ObjectKey {
        cache: CacheId::random(),
        pack: PackId::random(),
    };
    let store = app.engine.object_store().head(&probe).await.is_ok();
    if !(metadata && store) {
        let down: Vec<&str> = [(metadata, "metadata"), (store, "store")]
            .into_iter()
            .filter(|(ok, _)| !ok)
            .map(|(_, name)| name)
            .collect();
        let mut error = ApiError::from(Error::unavailable(down.join(", ")));
        error.message = format!("unavailable: {}", down.join(", "));
        return Err(error);
    }
    Ok(json_response(
        StatusCode::OK,
        &json!({
            "metadata": "ok",
            "store": "ok",
            "gc_leader": app.gc_leader(),
        }),
    ))
}

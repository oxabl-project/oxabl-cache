//! `GET|POST /v1/tenant/users`, `DELETE /v1/tenant/users/{id}`.

use crate::app::{App, Backend};
use crate::auth::Authed;
use crate::error::ApiError;
use crate::json::{empty, json_response, read_json};
use crate::path::principal_id;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use oxabl_cache_core::ports::metadata::User;
use oxabl_cache_core::{Conflict, Error};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewUser {
    issuer: String,
    subject: String,
    #[serde(default)]
    label: String,
}

fn user_json(user: &User) -> Value {
    json!({
        "id": user.id.to_string(),
        "issuer": user.issuer,
        "subject": user.subject,
        "label": user.label,
        "created_at": user.created_at,
        "disabled_at": user.disabled_at,
    })
}

pub(crate) async fn list<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
) -> Result<Response, ApiError> {
    let users = app.engine.list_users(&auth.caller).await?;
    let users: Vec<Value> = users.iter().map(user_json).collect();
    Ok(json_response(StatusCode::OK, &json!({ "users": users })))
}

/// Registers a user. Issuer and subject are the natural key: the identical request
/// again is a 200, a different label is a 409.
pub(crate) async fn create<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    let body: NewUser = read_json(body).await?;
    let created = app
        .engine
        .create_user(
            &auth.caller,
            body.issuer.clone(),
            body.subject.clone(),
            body.label.clone(),
        )
        .await;
    match created {
        Ok(id) => Ok(json_response(
            StatusCode::CREATED,
            &json!({ "id": id.to_string() }),
        )),
        Err(Error::Conflict(Conflict::Exists)) => {
            let existing = app
                .engine
                .list_users(&auth.caller)
                .await?
                .into_iter()
                .find(|user| user.issuer == body.issuer && user.subject == body.subject)
                .ok_or(Error::Conflict(Conflict::Exists))?;
            if existing.label == body.label && existing.disabled_at.is_none() {
                Ok(json_response(
                    StatusCode::OK,
                    &json!({ "id": existing.id.to_string() }),
                ))
            } else {
                Err(Error::Conflict(Conflict::Exists).into())
            }
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn delete<B: Backend>(
    State(app): State<Arc<App<B>>>,
    auth: Authed,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = principal_id(&id).map_err(|_| ApiError::not_found())?;
    app.engine.disable_user(&auth.caller, id).await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

//! Bearer authentication: tokens and sessions for the public API, operator tokens for admin.

use crate::app::{App, Backend};
use crate::ctx::RequestCtx;
use crate::error::ApiError;
use crate::path;
use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::principal::Caller;
use oxabl_cache_core::ids::CacheId;
use std::sync::Arc;

/// The bearer text of a request, if it is well formed.
pub(crate) fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(Error::Unauthenticated.into());
    };
    let text = value.to_str().map_err(|_| Error::Unauthenticated)?;
    let (scheme, token) = text.split_once(' ').ok_or(Error::Unauthenticated)?;
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() && !token.contains(' ') {
        Ok(token)
    } else {
        Err(Error::Unauthenticated.into())
    }
}

fn context(parts: &Parts) -> RequestCtx {
    parts
        .extensions
        .get::<RequestCtx>()
        .cloned()
        .unwrap_or_else(|| RequestCtx::new(String::new()))
}

/// An authenticated, rate-limited caller of the public API.
pub(crate) struct Authed {
    pub caller: Caller,
    pub ctx: RequestCtx,
}

impl Authed {
    /// Parses a cache id from the URL and records it for the access log.
    pub fn cache(&self, text: &str) -> Result<CacheId, ApiError> {
        let id = path::cache_id(text)?;
        self.ctx.set_cache(id.to_string());
        Ok(id)
    }
}

impl<B: Backend> FromRequestParts<Arc<App<B>>> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        app: &Arc<App<B>>,
    ) -> Result<Self, Self::Rejection> {
        let ctx = context(parts);
        let caller = app.engine.authenticate(bearer(&parts.headers)?).await?;
        ctx.set_tenant(caller.tenant.to_string(), caller.principal.to_string());
        app.limiter.admit(app, caller.tenant).await?;
        Ok(Self { caller, ctx })
    }
}

/// An authenticated service operator.
pub(crate) struct Operator {
    pub ctx: RequestCtx,
}

impl<B: Backend> FromRequestParts<Arc<App<B>>> for Operator {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        app: &Arc<App<B>>,
    ) -> Result<Self, Self::Rejection> {
        app.engine
            .authenticate_operator(bearer(&parts.headers)?)
            .await?;
        Ok(Self {
            ctx: context(parts),
        })
    }
}

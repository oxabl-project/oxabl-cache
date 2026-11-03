//! The routers of the three listeners.

mod admin;
mod cache_info;
mod commit;
mod dictionary;
mod health;
mod lookup;
mod metrics_page;
mod objects;
mod purge;
mod range;
mod reports;
mod roster;
mod served;
mod session;
mod tenant;
mod uploads;
mod urls;
mod versioned;

use crate::app::{App, Backend};
use crate::envelope::{envelope, label_route};
use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{delete, get, patch, post, put};
use std::sync::Arc;

fn wrap<B: Backend>(routes: Router<Arc<App<B>>>, app: Arc<App<B>>) -> Router {
    routes
        .route_layer(from_fn(label_route))
        .layer(from_fn_with_state(Arc::clone(&app), envelope::<B>))
        .with_state(app)
}

/// The public listener: authentication, the data plane and tenant administration.
pub fn public_router<B: Backend>(app: Arc<App<B>>) -> Router {
    let routes = Router::new()
        .route("/v1/health", get(health::public))
        .route("/v1/auth/oidc", post(session::oidc::<B>))
        .route("/v1/auth/session", delete(session::revoke::<B>))
        .route("/v1/caches/{cache}", get(cache_info::get::<B>))
        .route(
            "/v1/caches/{cache}/roster",
            get(roster::get::<B>).put(roster::put::<B>),
        )
        .route(
            "/v1/caches/{cache}/dictionary",
            get(dictionary::get::<B>).put(dictionary::put::<B>),
        )
        .route("/v1/caches/{cache}/lookup", post(lookup::post::<B>))
        .route("/v1/caches/{cache}/uploads", post(uploads::post::<B>))
        .route(
            "/v1/caches/{cache}/objects/{pack}",
            put(objects::put::<B>).get(objects::get::<B>),
        )
        .route(
            "/v1/caches/{cache}/packs/{pack}/commit",
            post(commit::post::<B>),
        )
        .route("/v1/caches/{cache}/urls", post(urls::post::<B>))
        .route("/v1/caches/{cache}/reports", post(reports::post::<B>))
        .route("/v1/caches/{cache}/purge", post(purge::post::<B>))
        .route(
            "/v1/tenant/caches",
            get(tenant::caches::list::<B>).post(tenant::caches::create::<B>),
        )
        .route(
            "/v1/tenant/caches/{cache}",
            delete(tenant::caches::delete::<B>),
        )
        .route(
            "/v1/tenant/caches/{cache}/owner-keys",
            get(tenant::owner_keys::get::<B>).put(tenant::owner_keys::put::<B>),
        )
        .route(
            "/v1/tenant/tokens",
            get(tenant::tokens::list::<B>).post(tenant::tokens::create::<B>),
        )
        .route(
            "/v1/tenant/tokens/{id}",
            delete(tenant::tokens::revoke::<B>),
        )
        .route(
            "/v1/tenant/users",
            get(tenant::users::list::<B>).post(tenant::users::create::<B>),
        )
        .route("/v1/tenant/users/{id}", delete(tenant::users::delete::<B>))
        .route(
            "/v1/tenant/grants",
            get(tenant::grants::list::<B>).put(tenant::grants::put::<B>),
        )
        .route(
            "/v1/tenant/oidc/issuers",
            get(tenant::oidc::list_issuers::<B>)
                .post(tenant::oidc::add_issuer::<B>)
                .delete(tenant::oidc::delete_issuer::<B>),
        )
        .route(
            "/v1/tenant/oidc/policies",
            get(tenant::oidc::list_policies::<B>)
                .post(tenant::oidc::add_policy::<B>)
                .delete(tenant::oidc::delete_policy::<B>),
        )
        .route("/v1/tenant/usage", get(tenant::usage::get::<B>));
    wrap(routes, app)
}

/// The operator listener.
pub fn admin_router<B: Backend>(app: Arc<App<B>>) -> Router {
    let routes = Router::new()
        .route(
            "/admin/v1/tenants",
            get(admin::tenants::list::<B>).post(admin::tenants::create::<B>),
        )
        .route(
            "/admin/v1/tenants/{tenant}",
            patch(admin::tenants::patch::<B>).delete(admin::tenants::delete::<B>),
        )
        .route(
            "/admin/v1/tenants/{tenant}/quota",
            put(admin::tenants::quota::<B>),
        )
        .route(
            "/admin/v1/tenants/{tenant}/tokens",
            post(admin::tenants::token::<B>),
        )
        .route(
            "/admin/v1/tenants/{tenant}/admins",
            post(admin::admins::post::<B>),
        )
        .route("/admin/v1/usage", get(admin::usage::get::<B>))
        .route(
            "/admin/v1/caches/{cache}/scrub",
            post(admin::scrub::post::<B>),
        )
        .route("/admin/v1/health", get(admin::health::get::<B>));
    wrap(routes, app)
}

/// The metrics listener.
pub fn metrics_router<B: Backend>(app: Arc<App<B>>) -> Router {
    wrap(
        Router::new()
            .route("/metrics", get(metrics_page::get::<B>))
            .route("/livez", get(health::live))
            .route("/readyz", get(health::ready::<B>)),
        app,
    )
}

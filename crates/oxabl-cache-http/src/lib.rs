//! HTTP surface of the cache server: the data plane, tenant and operator
//! administration, the error envelope, rate limiting, metrics and request logging.
//!
//! [`public_router`], [`admin_router`] and [`metrics_router`] build the three
//! listeners' routers over one shared [`App`].

mod app;
mod auth;
mod body;
mod ctx;
mod envelope;
mod error;
mod frames;
mod health;
mod json;
mod limiter;
pub mod log;
mod metered;
mod metrics;
mod path;
mod routes;

pub use app::{App, AppEngine, Backend, TenantDefaults};
pub use metered::{MeteredMeta, MeteredStore};
pub use metrics::Metrics;
pub use routes::{admin_router, metrics_router, public_router};

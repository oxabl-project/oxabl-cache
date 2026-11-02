//! Domain model, use cases and port traits of the cache server.
//!
//! The core performs no I/O of its own: metadata, object bytes, identity
//! verification and time arrive through the traits in [`ports`].

pub mod config;
pub mod domain;
pub mod encoding;
pub mod engine;
pub mod error;
pub mod ids;
pub mod limits;
pub mod ports;
pub mod state;
pub mod time;
pub mod usecases;

pub use config::Config;
pub use engine::Engine;
pub use error::{Conflict, Error, Result};

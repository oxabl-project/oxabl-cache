//! Everything the window shows that comes from the cache servers or the disk.

mod credentials;
mod identity;
mod join;
pub mod model;
pub mod paths;
mod refresh;
pub mod settings;
pub mod worker;

pub use join::machine_label;

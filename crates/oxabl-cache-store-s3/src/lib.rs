//! ObjectStore adapter on the portable S3 subset.
//!
//! Objects live at `<prefix>v1/<cache>/<pack>`. Clients move bytes through presigned URLs;
//! the adapter itself signs with `rusty-s3` and talks to the store over `reqwest`. Opening a
//! store runs a conformance probe and refuses to return one that fails a required check.

mod checksum;
mod config;
mod credentials;
mod delete;
mod delete_answer;
mod error;
mod head;
mod layout;
mod list;
mod object_store;
mod probe;
mod read;
mod signing;
mod store;
mod urls;
mod write;

pub use config::S3Config;
pub use store::S3Store;

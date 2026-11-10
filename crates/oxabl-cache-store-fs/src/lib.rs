//! ObjectStore adapter on a local directory.
//!
//! Objects live at `<root>/v1/<cache>/<pack[0..2]>/<pack>` and are written
//! through `<root>/tmp` with fsync, rename and a directory fsync. URLs have the
//! shape of presigned ones and are signed with HMAC-SHA256; the HTTP layer serves
//! them and checks them with [`FsStore::verify_url`].

mod base64url;
mod error;
mod head;
mod layout;
mod object_store;
mod store;
mod url_signing;
mod writer;

pub use store::{FsStore, UrlRequest};
pub use url_signing::{UrlRejection, object_path};
pub use writer::ObjectWriter;

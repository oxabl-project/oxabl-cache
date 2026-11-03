//! Presigned PUT and GET URLs.

use crate::checksum;
use crate::signing::Window;
use crate::store::S3Store;
use oxabl_cache_core::ports::SignedUrl;
use rusty_s3::S3Action;
use rusty_s3::actions::{GetObject, PutObject};

fn put_headers(size: u64, sha256: Option<&[u8; 32]>, if_none_match: bool) -> Vec<(String, String)> {
    let mut headers = vec![("content-length".to_owned(), size.to_string())];
    if if_none_match {
        headers.push(("if-none-match".to_owned(), "*".to_owned()));
    }
    if let Some(digest) = sha256 {
        headers.push((
            "x-amz-checksum-sha256".to_owned(),
            checksum::header_value(digest),
        ));
    }
    headers
}

impl S3Store {
    /// A PUT URL for `size` bytes at `key` whose headers are signed.
    pub(crate) fn put_signed(
        &self,
        key: &str,
        size: u64,
        sha256: Option<&[u8; 32]>,
        if_none_match: bool,
        window: &Window,
    ) -> SignedUrl {
        let headers = put_headers(size, sha256, if_none_match);
        let mut action = PutObject::new(&self.bucket, Some(&self.credentials), key);
        for (name, value) in &headers {
            action.headers_mut().insert(name.clone(), value.clone());
        }
        SignedUrl {
            url: window.sign(&action).to_string(),
            method: "PUT",
            headers,
            expires_at: window.expires_at,
        }
    }

    /// A GET URL at `key` that serves any range.
    pub(crate) fn get_signed(&self, key: &str, window: &Window) -> SignedUrl {
        let action = GetObject::new(&self.bucket, Some(&self.credentials), key);
        SignedUrl {
            url: window.sign(&action).to_string(),
            method: "GET",
            headers: Vec::new(),
            expires_at: window.expires_at,
        }
    }
}

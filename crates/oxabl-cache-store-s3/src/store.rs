//! The store handle: bucket, credentials, HTTP client and detected capabilities.

use crate::config::S3Config;
use crate::credentials;
use crate::layout::Layout;
use crate::probe;
use oxabl_cache_core::{Error, Result};
use reqwest::redirect::Policy;
use rusty_s3::{Bucket, Credentials, UrlStyle};
use std::fmt;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// The optional capabilities the probe found.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Optional {
    pub(crate) if_none_match: bool,
    pub(crate) checksum_sha256: bool,
}

/// An object store on an S3-compatible bucket. Cloning is cheap.
#[derive(Clone)]
pub struct S3Store {
    pub(crate) bucket: Bucket,
    pub(crate) credentials: Credentials,
    pub(crate) http: reqwest::Client,
    pub(crate) layout: Layout,
    pub(crate) optional: Optional,
}

impl fmt::Debug for S3Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Store")
            .field("bucket", &self.bucket.name())
            .field("optional", &self.optional)
            .finish_non_exhaustive()
    }
}

impl S3Store {
    /// Connects to the bucket and runs the conformance probe. A bucket that fails a
    /// required check, or cannot be reached, gives an error and no store.
    pub async fn open(config: &S3Config) -> Result<Self> {
        let mut store = Self::unprobed(config)?;
        store.optional = probe::run(&store).await?;
        Ok(store)
    }

    fn unprobed(config: &S3Config) -> Result<Self> {
        if !matches!(config.endpoint.scheme(), "http" | "https") {
            return Err(Error::bad_request("store.endpoint is an http or https URL"));
        }
        let style = if config.path_style {
            UrlStyle::Path
        } else {
            UrlStyle::VirtualHost
        };
        let bucket = Bucket::new(
            config.endpoint.clone(),
            style,
            config.bucket.clone(),
            config.region.clone(),
        )
        .map_err(|error| Error::bad_request(format!("store.endpoint: {error}")))?;
        let credentials =
            credentials::from_files(&config.access_key_id_file, &config.secret_access_key_file)?;
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(Policy::none())
            .build()
            .map_err(|error| Error::internal(format!("http client: {error}")))?;
        Ok(Self {
            bucket,
            credentials,
            http,
            layout: Layout::new(&config.prefix)?,
            optional: Optional::default(),
        })
    }
}

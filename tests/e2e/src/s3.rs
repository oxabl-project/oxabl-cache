//! A bucket on an S3-compatible server and the presigned operations the harness needs.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use rusty_s3::actions::{
    CreateBucket, DeleteObject, GetObject, HeadObject, ListObjectsV2, PutObject,
};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use url::Url;

const SIGNATURE_LIFETIME: Duration = Duration::from_secs(60);

/// Everything needed to talk to one bucket.
#[derive(Clone, Debug)]
pub struct S3Bucket {
    pub endpoint: Url,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
}

impl S3Bucket {
    fn bucket(&self) -> Result<Bucket> {
        Ok(Bucket::new(
            self.endpoint.clone(),
            UrlStyle::Path,
            self.bucket.clone(),
            self.region.clone(),
        )?)
    }

    fn credentials(&self) -> Credentials {
        Credentials::new(&self.access_key, &self.secret_key)
    }

    /// Creates the bucket with a presigned `CreateBucket`.
    pub async fn create(&self) -> Result<()> {
        let bucket = self.bucket()?;
        let credentials = self.credentials();
        let url = CreateBucket::new(&bucket, &credentials).sign(SIGNATURE_LIFETIME);
        let response = reqwest::Client::new().put(url).send().await?;
        expect_status(response, StatusCode::OK).await
    }

    /// Presigned PUT of `body` at `key`.
    pub async fn put(&self, key: &str, body: Vec<u8>) -> Result<()> {
        let bucket = self.bucket()?;
        let credentials = self.credentials();
        let url = PutObject::new(&bucket, Some(&credentials), key).sign(SIGNATURE_LIFETIME);
        let response = reqwest::Client::new().put(url).body(body).send().await?;
        expect_status(response, StatusCode::OK).await
    }

    /// Presigned GET of the inclusive byte range `first..=last` of `key`.
    pub async fn get_range(&self, key: &str, first: u64, last: u64) -> Result<Vec<u8>> {
        let bucket = self.bucket()?;
        let credentials = self.credentials();
        let url = GetObject::new(&bucket, Some(&credentials), key).sign(SIGNATURE_LIFETIME);
        let response = reqwest::Client::new()
            .get(url)
            .header("Range", format!("bytes={first}-{last}"))
            .send()
            .await?;
        if response.status() != StatusCode::PARTIAL_CONTENT {
            bail!("ranged GET answered {}", response.status());
        }
        Ok(response.bytes().await?.to_vec())
    }

    /// Presigned HEAD: the object's length, or `None` if it does not exist.
    pub async fn head(&self, key: &str) -> Result<Option<u64>> {
        let bucket = self.bucket()?;
        let credentials = self.credentials();
        let url = HeadObject::new(&bucket, Some(&credentials), key).sign(SIGNATURE_LIFETIME);
        let response = reqwest::Client::new().head(url).send().await?;
        match response.status() {
            StatusCode::OK => Ok(Some(
                response
                    .headers()
                    .get("content-length")
                    .context("HEAD answered without content-length")?
                    .to_str()?
                    .parse()?,
            )),
            StatusCode::NOT_FOUND => Ok(None),
            status => bail!("HEAD answered {status}"),
        }
    }

    /// Presigned listing of the keys under `prefix`, first page only.
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let bucket = self.bucket()?;
        let credentials = self.credentials();
        let mut action = bucket.list_objects_v2(Some(&credentials));
        action.with_prefix(prefix);
        let url = action.sign(SIGNATURE_LIFETIME);
        let response = reqwest::Client::new().get(url).send().await?;
        if response.status() != StatusCode::OK {
            bail!("list answered {}", response.status());
        }
        let page = ListObjectsV2::parse_response(&response.text().await?)?;
        Ok(page.contents.into_iter().map(|object| object.key).collect())
    }

    /// Presigned DELETE of `key`.
    pub async fn delete(&self, key: &str) -> Result<()> {
        let bucket = self.bucket()?;
        let credentials = self.credentials();
        let url = DeleteObject::new(&bucket, Some(&credentials), key).sign(SIGNATURE_LIFETIME);
        let response = reqwest::Client::new().delete(url).send().await?;
        if !response.status().is_success() {
            bail!("DELETE answered {}", response.status());
        }
        Ok(())
    }
}

async fn expect_status(response: reqwest::Response, expected: StatusCode) -> Result<()> {
    let status = response.status();
    if status != expected {
        bail!(
            "expected {expected}, got {status}: {}",
            response.text().await?
        );
    }
    Ok(())
}

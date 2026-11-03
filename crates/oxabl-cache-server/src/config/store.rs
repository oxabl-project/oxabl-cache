//! The `[store]` section: a local directory or an S3 bucket.

use super::raw;
use std::path::PathBuf;

/// Where pack bytes live.
#[derive(Debug, Clone)]
pub(crate) enum StoreConfig {
    /// A local directory served through the object route.
    Fs { root: PathBuf },
    /// An S3-compatible bucket reached through presigned URLs.
    S3(oxabl_cache_store_s3::S3Config),
}

fn bucket_name(name: &str) -> bool {
    (3..=63).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
}

fn needed<T>(value: Option<T>, key: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("store.{key} is required for kind s3"))
}

/// Validates the section for its kind; keys of the other kind are errors.
pub(crate) fn parse(raw: raw::Store) -> Result<StoreConfig, String> {
    let s3_keys = [
        raw.endpoint.is_some(),
        raw.region.is_some(),
        raw.bucket.is_some(),
        raw.prefix.is_some(),
        raw.path_style.is_some(),
        raw.access_key_id_file.is_some(),
        raw.secret_access_key_file.is_some(),
    ];
    match raw.kind.as_str() {
        "fs" => {
            if s3_keys.iter().any(|set| *set) {
                return Err("store keys for s3 are set but the store kind is fs".to_owned());
            }
            let root = raw.root.ok_or("store.root is required for kind fs")?;
            Ok(StoreConfig::Fs { root: root.into() })
        }
        "s3" => {
            if raw.root.is_some() {
                return Err("store.root applies to kind fs only".to_owned());
            }
            let endpoint = needed(raw.endpoint, "endpoint")?;
            let endpoint = endpoint
                .parse()
                .map_err(|_| format!("store.endpoint `{endpoint}` is not a URL"))?;
            let bucket = needed(raw.bucket, "bucket")?;
            if !bucket_name(&bucket) {
                return Err(format!(
                    "store.bucket `{bucket}` is 3 to 63 lowercase letters, digits, `-` and `.`"
                ));
            }
            Ok(StoreConfig::S3(oxabl_cache_store_s3::S3Config {
                endpoint,
                region: needed(raw.region, "region")?,
                bucket,
                prefix: raw.prefix.unwrap_or_default(),
                path_style: raw.path_style.unwrap_or(false),
                access_key_id_file: needed(raw.access_key_id_file, "access_key_id_file")?.into(),
                secret_access_key_file: needed(
                    raw.secret_access_key_file,
                    "secret_access_key_file",
                )?
                .into(),
            }))
        }
        other => Err(format!("store kind `{other}` is fs or s3")),
    }
}

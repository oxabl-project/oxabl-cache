use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use oxabl_cache_format::PackId;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;

use super::{Api, ApiError, BINARY};
use crate::progress::Progress;

const BODY_CAP: usize = 64 * 1024;

#[derive(Deserialize)]
struct Location {
    pack_id: String,
    url: String,
    offset: u64,
    length: u32,
}

#[derive(Deserialize)]
struct Raw {
    pointer: String,
    location: Location,
}

/// The dictionary pointer and where its segment lives.
pub(crate) struct DictionaryDocument {
    /// The `OXDP` bytes.
    pub pointer: Vec<u8>,
    pub pack: PackId,
    pub url: String,
    pub offset: u64,
    pub length: u32,
}

impl Api {
    /// The published dictionary, or `None` while none is.
    pub async fn dictionary(
        &self,
        progress: &Progress,
    ) -> Result<Option<DictionaryDocument>, ApiError> {
        let response = match self
            .call(progress, |c| c.get(self.url("/dictionary")))
            .await
        {
            Ok(response) => response,
            Err(e) if e.status() == Some(404) => return Ok(None),
            Err(e) => return Err(e),
        };
        let body = super::body::read_capped(response, BODY_CAP, progress, &self.counters).await?;
        let raw: Raw = serde_json::from_slice(&body).map_err(|_| ApiError::Malformed)?;
        Ok(Some(DictionaryDocument {
            pointer: STANDARD
                .decode(raw.pointer)
                .map_err(|_| ApiError::Malformed)?,
            pack: PackId::from_hex(&raw.location.pack_id).map_err(|_| ApiError::Malformed)?,
            url: raw.location.url,
            offset: raw.location.offset,
            length: raw.location.length,
        }))
    }
}

impl Api {
    /// Moves the dictionary pointer from version `current` (0 while there is none) to `pointer`.
    pub async fn put_dictionary_pointer(
        &self,
        current: u64,
        pointer: &Bytes,
    ) -> Result<(), ApiError> {
        self.call(&Progress::new(), |c| {
            c.put(self.url("/dictionary"))
                .header("If-Match", format!("\"v{current}\""))
                .header(CONTENT_TYPE, BINARY)
                .body(pointer.clone())
        })
        .await
        .map(|_| ())
    }
}

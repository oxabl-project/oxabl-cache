use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use oxabl_cache_format::PackId;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{Api, ApiError, BINARY};
use crate::progress::Progress;

const BODY_CAP: usize = 1024 * 1024;
const COMMIT_BODY_CAP: usize = 64 * 1024;

/// One pack to reserve.
pub(crate) struct UploadRequest {
    /// 16 random bytes in lowercase hex.
    pub client_ref: String,
    pub size: u64,
    /// The SHA-256 of the pack, sent when the store wants it.
    pub sha256: Option<[u8; 32]>,
}

/// The server's answer for one pack.
#[derive(Debug)]
pub(crate) enum UploadSlot {
    Ready {
        pack: PackId,
        url: String,
        headers: BTreeMap<String, String>,
    },
    Committed {
        pack: PackId,
    },
    QuotaExceeded,
}

#[derive(Deserialize)]
struct RawSlot {
    status: String,
    pack_id: Option<String>,
    url: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RawUploads {
    packs: Vec<RawSlot>,
}

fn slot(raw: RawSlot) -> Result<UploadSlot, ApiError> {
    let pack = || {
        raw.pack_id
            .as_deref()
            .and_then(|id| PackId::from_hex(id).ok())
            .ok_or(ApiError::Malformed)
    };
    match raw.status.as_str() {
        "ok" => Ok(UploadSlot::Ready {
            pack: pack()?,
            url: raw.url.clone().ok_or(ApiError::Malformed)?,
            headers: raw.headers.clone(),
        }),
        "committed" => Ok(UploadSlot::Committed { pack: pack()? }),
        "quota_exceeded" => Ok(UploadSlot::QuotaExceeded),
        _ => Err(ApiError::Malformed),
    }
}

impl Api {
    /// Reserves packs in a namespace; the answers come in request order.
    pub async fn uploads(
        &self,
        namespace: &str,
        packs: &[UploadRequest],
    ) -> Result<Vec<UploadSlot>, ApiError> {
        let body = serde_json::json!({
            "namespace": namespace,
            "packs": packs.iter().map(|p| {
                let mut pack = serde_json::json!({"client_ref": p.client_ref, "size": p.size});
                if let Some(sha) = p.sha256 {
                    pack["sha256"] = STANDARD.encode(sha).into();
                }
                pack
            }).collect::<Vec<_>>(),
        });
        let progress = Progress::new();
        let response = self
            .call(&progress, |c| c.post(self.url("/uploads")).json(&body))
            .await?;
        let bytes = super::body::read_capped(response, BODY_CAP, &progress, &self.counters).await?;
        let raw: RawUploads = serde_json::from_slice(&bytes).map_err(|_| ApiError::Malformed)?;
        if raw.packs.len() != packs.len() {
            return Err(ApiError::Malformed);
        }
        raw.packs.into_iter().map(slot).collect()
    }

    /// Posts the exact `OXCM` bytes of a pack's commit and returns the `OXCA` bytes.
    pub async fn commit(&self, pack: PackId, frame: &Bytes) -> Result<Vec<u8>, ApiError> {
        self.counters.sent(frame.len() as u64);
        let progress = Progress::new();
        let response = self
            .call(&progress, |c| {
                c.post(self.url(&format!("/packs/{}/commit", pack.to_hex())))
                    .header(CONTENT_TYPE, BINARY)
                    .body(frame.clone())
            })
            .await?;
        super::body::read_capped(response, COMMIT_BODY_CAP, &progress, &self.counters).await
    }
}

/// The SHA-256 of a pack.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

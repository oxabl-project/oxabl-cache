use bytes::Bytes;
use oxabl_cache_crypto::{seal_dictionary, sign_pointer};
use oxabl_cache_format::{DictionaryPointer, Role};

use crate::api::ApiError;
use crate::cache::Cache;
use crate::mode::Mode;
use crate::progress::Progress;
use crate::store::StoreFailure;
use crate::store::pack_build::build_dictionary;
use crate::store::publish::{Published, publish};

/// Why a dictionary was not published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DictionaryError {
    /// Only a `ci` member in `ci` mode publishes: the pointer must be signed by a `ci` key and
    /// the pack written to the trusted namespace.
    NotAWriter,
    /// The dictionary is empty or too large.
    Invalid,
    /// The server refused the pack or the pointer.
    Rejected,
    QuotaExceeded,
    /// Another pointer was published first: read the cache again and repeat.
    Stale,
    Unavailable,
}

impl std::fmt::Display for DictionaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotAWriter => "only a ci member in ci mode publishes a dictionary",
            Self::Invalid => "the dictionary is empty or too large",
            Self::Rejected => "the server refused the dictionary",
            Self::QuotaExceeded => "the cache is over its quota",
            Self::Stale => "another dictionary was published first; repeat",
            Self::Unavailable => "the cache server is unavailable",
        })
    }
}

impl std::error::Error for DictionaryError {}

/// A dictionary the server now points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryPublished {
    /// The sealed dictionary segment, in lower-case hex.
    pub segment_id: String,
    pub pointer_version: u64,
    /// Bytes uploaded.
    pub bytes: u64,
}

impl Cache {
    /// Seals `dictionary`, commits it in a trusted pack of its own and moves the cache's
    /// dictionary pointer to it, signed with this identity's `ci` key.
    pub async fn publish_dictionary(
        &self,
        dictionary: &[u8],
    ) -> Result<DictionaryPublished, DictionaryError> {
        let published = self.publish_dictionary_once(dictionary).await;
        if let Err(error) = &published {
            self.inner.errors.record(
                "dictionary",
                crate::error_log::dictionary_kind(error),
                &error.to_string(),
            );
        }
        published
    }

    async fn publish_dictionary_once(
        &self,
        dictionary: &[u8],
    ) -> Result<DictionaryPublished, DictionaryError> {
        let inner = &self.inner;
        let view = inner.view();
        if inner.mode != Mode::Ci || view.role(&inner.identity) != Some(Role::Ci) {
            return Err(DictionaryError::NotAWriter);
        }
        let mut segment = Vec::new();
        let seal = seal_dictionary(&view.segment, dictionary, &mut segment)
            .map_err(|_| DictionaryError::Invalid)?;
        let built = build_dictionary(inner.pins.cache, seal.id, &segment)
            .map_err(|_| DictionaryError::Invalid)?;
        let uploaded = match publish(&inner.api, "trusted", &built, 0, inner.checksum_sha256).await
        {
            Published::Done { uploaded, .. } => uploaded,
            Published::Failed(StoreFailure::QuotaExceeded) => {
                return Err(DictionaryError::QuotaExceeded);
            }
            Published::Failed(StoreFailure::Rejected | StoreFailure::TooLarge) => {
                return Err(DictionaryError::Rejected);
            }
            Published::Failed(_) => return Err(DictionaryError::Unavailable),
        };
        let current = match inner.api.dictionary(&Progress::new()).await {
            Ok(Some(document)) => {
                DictionaryPointer::decode(&document.pointer)
                    .map_err(|_| DictionaryError::Unavailable)?
                    .version
            }
            Ok(None) => 0,
            Err(_) => return Err(DictionaryError::Unavailable),
        };
        let version = current + 1;
        let pointer = sign_pointer(
            &inner.identity,
            inner.pins.cache,
            view.epoch(),
            version,
            seal.id,
        );
        inner
            .api
            .put_dictionary_pointer(current, &Bytes::from(pointer.encode()))
            .await
            .map_err(|e| match e {
                ApiError::Status(s) if s.status == 412 => DictionaryError::Stale,
                ApiError::Status(s) if matches!(s.status, 400 | 401 | 403) => {
                    DictionaryError::Rejected
                }
                _ => DictionaryError::Unavailable,
            })?;
        Ok(DictionaryPublished {
            segment_id: seal.id.to_hex(),
            pointer_version: version,
            bytes: uploaded,
        })
    }
}

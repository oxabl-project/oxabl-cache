//! The kind and the message each failure is logged with: fixed text and counts, never the
//! payload of an error, which may hold a path, a value of the configuration or a secret.

use crate::config::ConfigError;
use crate::dictionary::DictionaryError;
use crate::error::{CacheError, OpenError};
use crate::fetch::MissReason;
use crate::store::{StoreFailure, StoreOutcome};

fn config_message(error: &ConfigError) -> String {
    match error {
        ConfigError::MissingFile(_) => "the cache configuration file is missing".to_owned(),
        ConfigError::NotRegularFile(_) => {
            "the cache configuration is not a regular file in the repository".to_owned()
        }
        ConfigError::Io(kind) => format!("the cache configuration cannot be read: {kind}"),
        ConfigError::Parse(_) => "the cache configuration is not valid".to_owned(),
        ConfigError::UnknownKey(_) => "the cache configuration has an unknown key".to_owned(),
        ConfigError::Invalid(what) => format!("invalid cache configuration: {what}"),
        ConfigError::TrustOverride(variable) => {
            format!("{variable} cannot override a trust field")
        }
        ConfigError::Environment(variable) => format!("{variable} holds an unusable value"),
        ConfigError::InsecureServer => {
            "the server URL must be https, or http on a loopback address".to_owned()
        }
    }
}

/// The kind and message of a failed `Cache::open`.
pub(crate) fn open_failure(error: &OpenError) -> (&'static str, String) {
    match error {
        OpenError::Config(e) => ("config", config_message(e)),
        OpenError::NoCredentials => ("no_credentials", error.to_string()),
        OpenError::Identity(_) => ("identity", error.to_string()),
        OpenError::IdToken(_) => ("id_token", error.to_string()),
        OpenError::Unreachable => ("unreachable", error.to_string()),
        OpenError::Authentication => ("authentication", error.to_string()),
        OpenError::CacheNotFound => ("cache_not_found", error.to_string()),
        OpenError::Server(_) => ("server", error.to_string()),
        OpenError::BadResponse => ("bad_response", error.to_string()),
        OpenError::NoRoster => ("no_roster", error.to_string()),
        OpenError::Roster(_) => ("roster", "the roster failed verification".to_owned()),
        OpenError::IdentityNotInRoster => ("identity_not_in_roster", error.to_string()),
        OpenError::State(_) => ("state", error.to_string()),
    }
}

/// The kind of a failed operation on an open cache.
pub(crate) fn cache_kind(error: &CacheError) -> &'static str {
    match error {
        CacheError::ReadOnly => "read_only",
        CacheError::NotAWriter => "not_a_writer",
        CacheError::Unavailable => "unavailable",
        CacheError::SampleSize => "sample_size",
    }
}

/// The kind of a failed dictionary publication.
pub(crate) fn dictionary_kind(error: &DictionaryError) -> &'static str {
    match error {
        DictionaryError::NotAWriter => "not_a_writer",
        DictionaryError::Invalid => "invalid",
        DictionaryError::Rejected => "rejected",
        DictionaryError::QuotaExceeded => "quota_exceeded",
        DictionaryError::Stale => "stale",
        DictionaryError::Unavailable => "unavailable",
    }
}

/// The kind and the meaning of a miss that is a failure; a plain `NotFound` is not one.
pub(crate) fn fetch_failure(reason: MissReason) -> Option<(String, &'static str)> {
    let (kind, meaning) = match reason {
        MissReason::NotFound => return None,
        MissReason::Untrusted => ("untrusted", "signed by a key the roster does not accept"),
        MissReason::Corrupt => ("corrupt", "failed verification"),
        MissReason::PathMismatch => ("path_mismatch", "hold other files than expected"),
        MissReason::Timeout => ("timeout", "made no progress in time"),
        MissReason::Unavailable => ("unavailable", "could not be fetched from the cache"),
        MissReason::Install(kind) => {
            let kind = kind.to_string().replace(' ', "_");
            return Some((format!("install_{kind}"), "could not be installed"));
        }
    };
    Some((kind.to_owned(), meaning))
}

/// The kind and the meaning of a failed store.
fn store_failure(failure: StoreFailure) -> (&'static str, &'static str) {
    match failure {
        StoreFailure::Rejected => ("rejected", "were refused"),
        StoreFailure::TooLarge => ("too_large", "exceed a size limit"),
        StoreFailure::Unreadable => ("unreadable", "have a file that cannot be read"),
        StoreFailure::QuotaExceeded => (
            "quota_exceeded",
            "were not stored: the cache is over its quota",
        ),
        StoreFailure::Unavailable => ("unavailable", "could not be uploaded to the cache"),
    }
}

/// The kind and the meaning of a store outcome that is a failure.
pub(crate) fn store_outcome_failure(
    outcome: &StoreOutcome,
) -> Option<(&'static str, &'static str)> {
    match outcome {
        StoreOutcome::Committed | StoreOutcome::Existed => None,
        StoreOutcome::Changed => Some(("changed", "had a file that changed while it was stored")),
        StoreOutcome::Failed(failure) => Some(store_failure(*failure)),
    }
}

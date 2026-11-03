use std::time::Duration;

use oxabl_cache_format::{CommitResponse, CommitStatus, PackId};
use tracing::{debug, warn};

use super::pack_build::BuiltPack;
use super::types::{StoreFailure, StoreOutcome};
use crate::api::{Api, ApiError, UploadRequest, UploadSlot, sha256};

const RETRIES: u32 = 3;
const BACKOFF: Duration = Duration::from_millis(200);
const BACKOFF_CAP: Duration = Duration::from_secs(5);

/// How a pack's publication ended.
pub(crate) enum Published {
    /// The commit answered; one outcome per entry, in order. `uploaded` is the bytes PUT.
    Done {
        outcomes: Vec<StoreOutcome>,
        uploaded: u64,
    },
    Failed(StoreFailure),
}

enum Placed {
    Pack { id: PackId, uploaded: u64 },
    Quota,
    Failed(StoreFailure),
}

fn fresh_ref() -> String {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).expect("operating system randomness");
    random.iter().map(|b| format!("{b:02x}")).collect()
}

async fn pause(attempt: u32, hint: Option<Duration>) {
    let wait = hint.unwrap_or(BACKOFF * 2u32.pow(attempt)).min(BACKOFF_CAP);
    tokio::time::sleep(wait).await;
}

/// The failure a refused request maps to, or `None` when trying again may help.
fn refusal(error: &ApiError) -> Option<StoreFailure> {
    match error.status() {
        Some(400 | 403) => Some(StoreFailure::Rejected),
        Some(413) => Some(StoreFailure::TooLarge),
        _ if error.retryable() => None,
        _ => Some(StoreFailure::Unavailable),
    }
}

/// Reserves the pack and writes it, repeating both on a failure so that a fresh URL replaces
/// an expired one. The same `client_ref` is kept until the server closes it.
async fn place(api: &Api, namespace: &str, built: &BuiltPack, checksum: bool) -> Placed {
    let size = built.object.len() as u64;
    let digest = checksum.then(|| sha256(&built.object));
    let mut client_ref = fresh_ref();
    for attempt in 0..=RETRIES {
        let request = UploadRequest {
            client_ref: client_ref.clone(),
            size,
            sha256: digest,
        };
        let hint = match api.uploads(namespace, &[request]).await {
            Ok(slots) => match slots.into_iter().next() {
                Some(UploadSlot::Ready { pack, url, headers }) => {
                    match api.put_object(&url, &headers, &built.object).await {
                        Ok(()) => {
                            return Placed::Pack {
                                id: pack,
                                uploaded: size,
                            };
                        }
                        Err(error) => {
                            debug!(?error, "the pack write failed");
                            None
                        }
                    }
                }
                Some(UploadSlot::Committed { pack }) => {
                    return Placed::Pack {
                        id: pack,
                        uploaded: 0,
                    };
                }
                Some(UploadSlot::QuotaExceeded) => return Placed::Quota,
                None => return Placed::Failed(StoreFailure::Unavailable),
            },
            Err(error) if error.status() == Some(409) => {
                client_ref = fresh_ref();
                continue;
            }
            Err(error) => {
                if let Some(failure) = refusal(&error) {
                    return Placed::Failed(failure);
                }
                error.retry_after()
            }
        };
        if attempt < RETRIES {
            pause(attempt, hint).await;
        }
    }
    Placed::Failed(StoreFailure::Unavailable)
}

/// Sends the commit, resending the identical bytes when no answer arrives.
async fn commit(
    api: &Api,
    pack: PackId,
    built: &BuiltPack,
) -> Result<Vec<CommitStatus>, StoreFailure> {
    for attempt in 0..=RETRIES {
        match api.commit(pack, &built.commit).await {
            Ok(bytes) => {
                return CommitResponse::decode(&bytes)
                    .map(|r| r.statuses)
                    .map_err(|_| StoreFailure::Unavailable);
            }
            Err(error) if error.status() == Some(409) => {
                warn!(
                    pack = %pack.to_hex(),
                    reason = error.reason().unwrap_or("conflict"),
                    "the server refused the commit of a pack"
                );
                return Err(StoreFailure::Unavailable);
            }
            Err(error) => {
                if let Some(failure) = refusal(&error) {
                    return Err(failure);
                }
                if attempt < RETRIES {
                    pause(attempt, error.retry_after()).await;
                }
            }
        }
    }
    Err(StoreFailure::Unavailable)
}

fn outcome(status: CommitStatus) -> StoreOutcome {
    match status {
        CommitStatus::Committed => StoreOutcome::Committed,
        CommitStatus::Exists => StoreOutcome::Existed,
        CommitStatus::Invalid | CommitStatus::MissingDictionary => {
            StoreOutcome::Failed(StoreFailure::Rejected)
        }
    }
}

/// Reserves, writes and commits one built pack of `entries` entries.
pub(crate) async fn publish(
    api: &Api,
    namespace: &str,
    built: &BuiltPack,
    entries: usize,
    checksum: bool,
) -> Published {
    let (id, uploaded) = match place(api, namespace, built, checksum).await {
        Placed::Pack { id, uploaded } => (id, uploaded),
        Placed::Quota => return Published::Failed(StoreFailure::QuotaExceeded),
        Placed::Failed(failure) => return Published::Failed(failure),
    };
    match commit(api, id, built).await {
        Ok(statuses) if statuses.len() == entries => Published::Done {
            outcomes: statuses.into_iter().map(outcome).collect(),
            uploaded,
        },
        Ok(_) => Published::Failed(StoreFailure::Unavailable),
        Err(failure) => Published::Failed(failure),
    }
}

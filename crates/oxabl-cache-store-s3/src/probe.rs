//! The startup conformance probe.
//!
//! Required checks (a failure refuses the store): presigned PUT and GET, ETag on HEAD that
//! changes with the bytes, ranged read, list, batch delete. Optional checks only set the
//! capability flags: `If-None-Match: *` and `x-amz-checksum-sha256` on a presigned PUT.

use crate::checksum;
use crate::error::{expect_success, transport};
use crate::signing::Window;
use crate::store::{Optional, S3Store};
use oxabl_cache_core::encoding::hex_encode;
use oxabl_cache_core::{Error, Result};
use reqwest::StatusCode;

const OBJECT_BYTES: usize = 1024;

fn random_bytes(len: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).map_err(|error| Error::internal(format!("randomness: {error}")))?;
    Ok(bytes)
}

fn fail(check: &str, detail: impl std::fmt::Display) -> Error {
    Error::unavailable(format!("store probe failed at {check}: {detail}"))
}

fn step<T>(check: &str, outcome: Result<T>) -> Result<T> {
    outcome.map_err(|error| match error {
        Error::Unavailable(detail) => fail(check, detail),
        other => fail(check, other),
    })
}

/// Runs every check under a fresh prefix of its own and removes what it wrote.
pub(crate) async fn run(store: &S3Store) -> Result<Optional> {
    let mut run = [0u8; 8];
    getrandom::fill(&mut run).map_err(|error| Error::internal(format!("randomness: {error}")))?;
    let prefix = store.layout.probe_prefix(&hex_encode(&run));
    let mut written = Vec::new();
    let outcome = checks(store, &prefix, &mut written).await;
    let cleanup = step(
        "batch delete of the probe objects",
        store.delete_keys(&written).await,
    );
    let optional = outcome?;
    cleanup?;
    Ok(optional)
}

async fn checks(store: &S3Store, prefix: &str, written: &mut Vec<String>) -> Result<Optional> {
    required(store, prefix, written).await?;
    let if_none_match = step(
        "If-None-Match",
        if_none_match(store, &format!("{prefix}conditional"), written).await,
    )?;
    let checksum_sha256 = step(
        "x-amz-checksum-sha256",
        checksum_sha256(store, &format!("{prefix}checksum"), written).await,
    )?;
    Ok(Optional {
        if_none_match,
        checksum_sha256,
    })
}

async fn put_plain(store: &S3Store, key: &str, bytes: Vec<u8>) -> Result<()> {
    let size = u64::try_from(bytes.len()).map_err(|_| Error::TooLarge)?;
    let signed = store.put_signed(key, size, None, false, &Window::own_call());
    let response = store.send_put(&signed, bytes).await?;
    expect_success("PUT", response).await.map(drop)
}

async fn required(store: &S3Store, prefix: &str, written: &mut Vec<String>) -> Result<()> {
    let key = format!("{prefix}object");
    let body = random_bytes(OBJECT_BYTES)?;
    let size = u64::try_from(body.len()).map_err(|_| Error::TooLarge)?;

    written.push(key.clone());
    step("presigned PUT", put_plain(store, &key, body.clone()).await)?;

    let head = step("HEAD", store.head_key(&key).await)?
        .ok_or_else(|| fail("HEAD", "the object just written does not exist"))?;
    if head.size != size {
        return Err(fail("HEAD", "the size differs from the bytes written"));
    }

    let signed = store.get_signed(&key, &Window::own_call());
    let fetched = step(
        "presigned GET",
        async {
            let response = store
                .http
                .get(&signed.url)
                .send()
                .await
                .map_err(transport)?;
            let response = expect_success("GET", response).await?;
            response.bytes().await.map_err(transport)
        }
        .await,
    )?;
    if fetched[..] != body[..] {
        return Err(fail("presigned GET", "the bytes differ from those written"));
    }

    let ranged = step(
        "ranged read",
        store.read_key_range(&key, 100, 200, None).await,
    )?;
    if ranged[..] != body[100..300] {
        return Err(fail(
            "ranged read",
            "the range differs from the bytes written",
        ));
    }

    let replacement = random_bytes(OBJECT_BYTES)?;
    step("overwrite", put_plain(store, &key, replacement).await)?;
    let changed = step("HEAD after overwrite", store.head_key(&key).await)?
        .ok_or_else(|| fail("HEAD after overwrite", "the object disappeared"))?;
    if changed.etag == head.etag {
        return Err(fail("ETag", "it does not change when the bytes change"));
    }

    let listed = step("list", store.list_prefix(prefix).await)?;
    if !listed
        .iter()
        .any(|(listed, listed_size, _)| *listed == key && *listed_size == size)
    {
        return Err(fail("list", "the object just written is not listed"));
    }

    let batch: Vec<String> = (0..3).map(|n| format!("{prefix}batch-{n}")).collect();
    written.extend(batch.iter().cloned());
    for name in &batch {
        step(
            "presigned PUT",
            put_plain(store, name, random_bytes(64)?).await,
        )?;
    }
    let mut doomed = batch.clone();
    doomed.push(key);
    step("batch delete", store.delete_keys(&doomed).await)?;
    step(
        "batch delete of missing keys",
        store.delete_keys(&doomed).await,
    )?;
    for name in &doomed {
        if step("HEAD after delete", store.head_key(name).await)?.is_some() {
            return Err(fail("batch delete", "an object survived"));
        }
    }
    Ok(())
}

async fn if_none_match(store: &S3Store, key: &str, written: &mut Vec<String>) -> Result<bool> {
    written.push(key.to_owned());
    let body = random_bytes(64)?;
    let size = u64::try_from(body.len()).map_err(|_| Error::TooLarge)?;
    let signed = store.put_signed(key, size, None, true, &Window::own_call());
    let first = store.send_put(&signed, body.clone()).await?;
    if !first.status().is_success() {
        return Ok(false);
    }
    let signed = store.put_signed(key, size, None, true, &Window::own_call());
    let second = store.send_put(&signed, random_bytes(64)?).await?;
    if second.status() != StatusCode::PRECONDITION_FAILED {
        return Ok(false);
    }
    let kept = store.read_key_range(key, 0, size, None).await?;
    Ok(kept == body)
}

async fn checksum_sha256(store: &S3Store, key: &str, written: &mut Vec<String>) -> Result<bool> {
    written.push(key.to_owned());
    let body = random_bytes(64)?;
    let size = u64::try_from(body.len()).map_err(|_| Error::TooLarge)?;
    let wrong = checksum::sha256(&random_bytes(64)?);
    let signed = store.put_signed(key, size, Some(&wrong), false, &Window::own_call());
    let refused = store.send_put(&signed, body.clone()).await?;
    if !refused.status().is_client_error() || store.head_key(key).await?.is_some() {
        return Ok(false);
    }
    let right = checksum::sha256(&body);
    let signed = store.put_signed(key, size, Some(&right), false, &Window::own_call());
    let accepted = store.send_put(&signed, body).await?;
    Ok(accepted.status().is_success())
}

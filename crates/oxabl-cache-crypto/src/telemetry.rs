//! Sealed telemetry reports: AES-256-GCM under a key derived from the cache root key, bound to
//! the cache and the epoch, so only members of that epoch read them and the server reads none.

use oxabl_cache_format::{CacheId, SealedTelemetry, TelemetryReport};

use crate::aead::Cipher;
use crate::domain::{AD_TELEMETRY, CTX_TELEMETRY, prefixed};
use crate::error::{Error, Result};
use crate::keys::RootKey;
use crate::random;

fn cipher(root: &RootKey) -> Result<Cipher> {
    Cipher::new(&blake3::derive_key(CTX_TELEMETRY, root.bytes()))
}

fn associated(cache: &CacheId, header: &[u8]) -> Vec<u8> {
    let mut bound = prefixed(AD_TELEMETRY, &cache.0);
    bound.extend_from_slice(header);
    bound
}

/// Seals `report` for the members of `cache` at `epoch` as an `OXTL` frame.
pub fn seal_telemetry(
    root: &RootKey,
    cache: &CacheId,
    epoch: u32,
    report: &TelemetryReport,
) -> Result<Vec<u8>> {
    let nonce = random::array()?;
    let header = SealedTelemetry::header(epoch, &nonce);
    let mut body = report.encode();
    cipher(root)?.seal(nonce, &associated(cache, &header), &mut body)?;
    let mut frame = header;
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// The epoch an `OXTL` frame was sealed in, read without a key.
pub fn telemetry_epoch(frame: &[u8]) -> Result<u32> {
    Ok(SealedTelemetry::decode(frame)?.epoch)
}

/// Opens an `OXTL` frame sealed for `cache` under the root key of `epoch`.
pub fn open_telemetry(
    root: &RootKey,
    cache: &CacheId,
    epoch: u32,
    frame: &[u8],
) -> Result<TelemetryReport> {
    let sealed = SealedTelemetry::decode(frame)?;
    if sealed.epoch != epoch {
        return Err(Error::Mismatch("telemetry of another epoch"));
    }
    let header = SealedTelemetry::header(sealed.epoch, &sealed.nonce);
    let mut body = sealed.sealed.to_vec();
    let plain = cipher(root)?.open(sealed.nonce, &associated(cache, &header), &mut body)?;
    Ok(TelemetryReport::decode(plain)?)
}

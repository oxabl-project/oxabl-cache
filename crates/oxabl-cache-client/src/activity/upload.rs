use std::sync::Arc;
use std::sync::atomic::Ordering;

use oxabl_cache_crypto::seal_telemetry;

use super::journal::unhex;
use super::{Activity, now_secs};
use crate::cache::Inner;

/// Reports older than this are kept in the journal but never sent.
const SEND_WITHIN_SECS: u64 = 7 * 24 * 60 * 60;
/// Reports sent per upload; the rest wait for the next one.
const SEND_AT_MOST: usize = 64;

/// Journals `activity` and sends what is pending in the background. Never fails, never waits.
pub(crate) fn record(inner: &Arc<Inner>, mut activity: Activity) {
    activity.rtt_ms = inner.rtt_ms.load(Ordering::Relaxed);
    let mut id = [0u8; 16];
    if getrandom::fill(&mut id).is_err() {
        return;
    }
    inner.journal.add(id, activity);
    inner.handle.spawn(upload_pending(inner.clone()));
}

/// Seals and sends the journal's unsent reports, oldest first, and marks them sent. Stops at
/// the first failure; the rest are sent by a later upload.
pub(crate) async fn upload_pending(inner: Arc<Inner>) {
    let _turn = inner.uploading.lock().await;
    if inner.rtt_ms.load(Ordering::Relaxed) == 0 {
        measure_round_trip(&inner).await;
    }
    let rtt_ms = inner.rtt_ms.load(Ordering::Relaxed);
    let since = now_secs().saturating_sub(SEND_WITHIN_SECS);
    let pending: Vec<_> = inner
        .journal
        .entries()
        .into_iter()
        .filter(|e| !e.sent && e.activity.at >= since)
        .take(SEND_AT_MOST)
        .collect();
    let view = inner.view();
    let mut sent = Vec::new();
    for entry in pending {
        let Some(id) = unhex(&entry.id) else { continue };
        let mut activity = entry.activity;
        if activity.rtt_ms == 0 {
            activity.rtt_ms = rtt_ms;
        }
        let Ok(frame) = seal_telemetry(
            &view.root,
            &inner.pins.cache,
            view.epoch(),
            &activity.report(id),
        ) else {
            break;
        };
        if inner.api.send_telemetry(frame).await.is_err() {
            break;
        }
        sent.push(entry.id);
    }
    if !sent.is_empty() {
        inner.journal.mark_sent(&sent);
    }
}

/// Measures the round trip to the server, for the reports that follow.
async fn measure_round_trip(inner: &Inner) {
    if let Ok(elapsed) = inner.api.round_trip().await {
        let ms = u32::try_from(elapsed.as_millis())
            .unwrap_or(u32::MAX)
            .max(1);
        inner.rtt_ms.store(ms, Ordering::Relaxed);
    }
}

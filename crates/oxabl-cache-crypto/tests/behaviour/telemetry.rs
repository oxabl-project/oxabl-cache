use oxabl_cache_crypto::{Error, RootKey, open_telemetry, seal_telemetry, telemetry_epoch};
use oxabl_cache_format::{CacheId, TelemetryKind, TelemetryMode, TelemetryReport};

fn report() -> TelemetryReport {
    TelemetryReport {
        id: [9; 16],
        at: 1_793_000_000,
        kind: TelemetryKind::Pull,
        mode: TelemetryMode::Developer,
        requested: 1_234,
        hits: 1_200,
        misses: 30,
        failed: 4,
        bytes: 987_654_321,
        duration_ms: 4_321,
        rtt_ms: 17,
    }
}

const CACHE: CacheId = CacheId([5; 16]);

#[test]
fn a_member_of_the_epoch_opens_what_another_member_sealed() {
    let root = RootKey::from_bytes([1; 32]);
    let frame = seal_telemetry(&root, &CACHE, 3, &report()).expect("seal");
    assert_eq!(telemetry_epoch(&frame).expect("epoch"), 3);
    assert_eq!(
        open_telemetry(&root, &CACHE, 3, &frame).expect("open"),
        report()
    );
    let again = seal_telemetry(&root, &CACHE, 3, &report()).expect("seal");
    assert_ne!(frame, again, "every seal takes a fresh nonce");
}

#[test]
fn the_frame_shows_nothing_but_its_epoch() {
    let root = RootKey::from_bytes([1; 32]);
    let frame = seal_telemetry(&root, &CACHE, 3, &report()).expect("seal");
    let plain = report().encode();
    for field in [&plain[..16], &plain[16..24], &plain[26..30], &plain[42..50]] {
        assert!(
            !frame.windows(field.len()).any(|w| w == field),
            "a plaintext field shows in the frame"
        );
    }
}

#[test]
fn another_key_cache_epoch_or_any_changed_byte_does_not_open() {
    let root = RootKey::from_bytes([1; 32]);
    let frame = seal_telemetry(&root, &CACHE, 3, &report()).expect("seal");
    let other_root = RootKey::from_bytes([2; 32]);
    assert_eq!(
        open_telemetry(&other_root, &CACHE, 3, &frame),
        Err(Error::Authentication)
    );
    assert_eq!(
        open_telemetry(&root, &CacheId([6; 16]), 3, &frame),
        Err(Error::Authentication),
        "a report replayed into another cache"
    );
    assert!(open_telemetry(&root, &CACHE, 4, &frame).is_err());
    let mut relabelled = frame.clone();
    relabelled[9] ^= 1;
    assert_eq!(
        open_telemetry(&root, &CACHE, 2, &relabelled),
        Err(Error::Authentication),
        "the epoch in the header is authenticated"
    );
    for at in 10..frame.len() {
        let mut changed = frame.clone();
        changed[at] ^= 0x40;
        assert!(
            open_telemetry(&root, &CACHE, 3, &changed).is_err(),
            "byte {at}"
        );
    }
    assert!(open_telemetry(&root, &CACHE, 3, &frame[..frame.len() - 1]).is_err());
}

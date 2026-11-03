use oxabl_cache_crypto::{open_dictionary, seal_dictionary, sealed_segment_len};
use oxabl_cache_format::SegmentId;

use crate::segments::root;
use crate::support::data::Rng;
use crate::support::reference;

fn log_uniform_sizes(count: usize, low: f64, high: f64, seed: u64) -> Vec<u64> {
    let mut rng = Rng::new(seed);
    (0..count)
        .map(|_| {
            let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
            (low * (high / low).powf(u)) as u64
        })
        .collect()
}

#[test]
fn sealed_length_follows_the_padme_rule_exactly() {
    let root = root();
    let mut rng = Rng::new(60);
    let mut sizes: Vec<usize> = vec![
        1, 2, 3, 50, 65_521, 65_522, 65_523, 65_535, 65_536, 131_000, 262_000,
    ];
    sizes.extend((0..24).map(|_| rng.below(400_000) as usize + 1));
    for n in sizes {
        let data = rng.bytes(n);
        let mut sealed = Vec::new();
        let info = seal_dictionary(&root.segment_key(), &data, &mut sealed).expect("seal");
        let padded = reference::padme(14 + n);
        let chunks = padded.div_ceil(reference::CHUNK);
        assert_eq!(
            sealed.len(),
            reference::SEGMENT_HEADER + padded + chunks * reference::TAG,
            "n = {n}"
        );
        assert_eq!(info.sealed_len, sealed.len() as u64);
        assert_eq!(
            sealed_segment_len(n as u64).expect("len"),
            sealed.len() as u64
        );
        let opened = open_dictionary(&root.segment_key(), &sealed, &info.id, 3).expect("open");
        assert_eq!(opened.as_bytes(), &data[..]);
        assert_eq!(opened.id(), info.id);
        assert_ne!(info.id, SegmentId([0; 32]));
    }
}

#[test]
fn padding_overhead_and_class_count_match_the_measured_security_figures() {
    let sizes = log_uniform_sizes(100_000, 4_096.0, 1_400_000.0, 61);
    let distinct: std::collections::BTreeSet<u64> = sizes.iter().copied().collect();
    let classes: std::collections::BTreeSet<u64> = sizes
        .iter()
        .map(|&n| sealed_segment_len(n).expect("len"))
        .collect();
    let plain: u64 = sizes.iter().sum();
    let sealed: u64 = sizes
        .iter()
        .map(|&n| sealed_segment_len(n).expect("len"))
        .sum();
    let overhead = (sealed - plain) as f64 / plain as f64 * 100.0;

    println!(
        "distinct sizes {}, classes {}, overhead {overhead:.2}%",
        distinct.len(),
        classes.len()
    );
    assert!(
        (74_000..=83_000).contains(&distinct.len()),
        "the distribution is the documented one: {} distinct sizes against 78,408",
        distinct.len()
    );
    assert!(
        (194..=214).contains(&classes.len()),
        "{} classes against the documented 204",
        classes.len()
    );
    assert!(
        (1.0..=1.5).contains(&overhead),
        "{overhead:.2}% extra bytes against the documented 1.2%"
    );
}

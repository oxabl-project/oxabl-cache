//! Throughput of the security stack on synthetic r-code-like sets: a scaled-down version of the
//! 4.3 GB workload in the security cost table.
//!
//! `cargo run --release -p oxabl-cache-crypto --example throughput -- --mb 256 --threads 8`

use std::hint::black_box;
use std::time::Instant;

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use oxabl_cache_crypto::{
    EntryFields, FileEntry, Identity, RecordContext, RootKey, SealOptions, SegmentExpectation,
    open_record, open_segment, seal_compressed, seal_record, seal_segment,
};
use oxabl_cache_format::{CacheId, Codec, Limits, LookupId, Namespace, SegmentId};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

const WORDS: [&str; 10] = [
    "ar/customer.p",
    "INTEGER",
    "CHARACTER",
    "FIND FIRST",
    "BUFFER-COPY",
    "RETURN ERROR",
    "src/common/",
    "DEFINE VARIABLE",
    "ttOrderLine",
    "Customer not found",
];

fn rcode_like(rng: &mut Rng, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 64);
    out.extend_from_slice(&[0x56, 0x6b, 0, 0, 0x0c, 0x08]);
    while out.len() < len {
        match rng.next() % 10 {
            0..=5 => out.extend_from_slice(WORDS[(rng.next() % 10) as usize].as_bytes()),
            6..=7 => out.extend_from_slice(&rng.next().to_le_bytes()),
            _ => out.extend_from_slice(&(rng.next() as u32).to_be_bytes()),
        }
        out.push(0);
    }
    out.truncate(len);
    out
}

fn log_uniform(rng: &mut Rng, low: f64, high: f64) -> usize {
    let u = (rng.next() >> 11) as f64 / (1u64 << 53) as f64;
    (low * (high / low).powf(u)) as usize
}

fn sets(total: usize) -> Vec<Vec<u8>> {
    let mut rng = Rng(7);
    let mut out = Vec::new();
    let mut sum = 0;
    while sum < total {
        let len = log_uniform(&mut rng, 4_096.0, 7.0 * 1024.0 * 1024.0)
            .min(total - sum)
            .max(1);
        out.push(rcode_like(&mut rng, len));
        sum += len;
    }
    out
}

fn parallel<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let per = items.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(per)
            .map(|part| scope.spawn(|| part.iter().map(&work).collect::<Vec<R>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("worker"))
            .collect()
    })
}

fn mbps(bytes: usize, seconds: f64) -> f64 {
    bytes as f64 / 1e6 / seconds
}

fn report(stage: &str, bytes: usize, single: f64, multi: f64, threads: usize) {
    println!(
        "{stage:<34} {:>9.0} MB/s single   {:>9.0} MB/s on {threads} threads",
        mbps(bytes, single),
        mbps(bytes, multi)
    );
}

fn timed<R>(f: impl FnOnce() -> R) -> (R, f64) {
    let start = Instant::now();
    let r = f();
    (r, start.elapsed().as_secs_f64())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str, default: usize| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let total = arg("--mb", 256) * 1_000_000;
    let threads = arg(
        "--threads",
        std::thread::available_parallelism().map_or(1, usize::from),
    );

    let sets = sets(total);
    let plain: usize = sets.iter().map(Vec::len).sum();
    println!(
        "{} sets, {:.1} MB plaintext, {threads} threads\n",
        sets.len(),
        plain as f64 / 1e6
    );

    let root = RootKey::from_bytes([9; 32]);
    let key = root.segment_key();
    let limits = Limits::default();

    let (_, t1) = timed(|| {
        sets.iter().for_each(|s| {
            black_box(blake3::hash(s));
        })
    });
    let (_, tn) = timed(|| parallel(&sets, threads, |s| blake3::hash(s)));
    report("BLAKE3 (reference)", plain, t1, tn, threads);

    let aes = |s: &Vec<u8>| {
        let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &[1; 32]).expect("key"));
        let mut buf = Vec::with_capacity(65_552);
        for (i, chunk) in s.chunks(65_536).enumerate() {
            buf.clear();
            buf.extend_from_slice(chunk);
            let mut nonce = [0u8; 12];
            nonce[3..11].copy_from_slice(&(i as u64).to_be_bytes());
            k.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut buf)
                .expect("seal");
        }
    };
    let (_, t1) = timed(|| sets.iter().for_each(aes));
    let (_, tn) = timed(|| parallel(&sets, threads, aes));
    report("AES-256-GCM chunks (aws-lc-rs)", plain, t1, tn, threads);

    let raw_seal = |s: &Vec<u8>| {
        let mut out = Vec::with_capacity(s.len() + 4096);
        let info = seal_compressed(&key, Codec::None, s, &mut out).expect("seal");
        (out, info)
    };
    let (sealed_raw, t1) = timed(|| sets.iter().map(raw_seal).collect::<Vec<_>>());
    let (_, tn) = timed(|| parallel(&sets, threads, raw_seal));
    report("seal, no compression (AES + id)", plain, t1, tn, threads);

    drop(sealed_raw);

    let seal = |s: &Vec<u8>| {
        let mut out = Vec::new();
        let info =
            seal_segment(&key, &s[..], &SealOptions::default(), &limits, &mut out).expect("seal");
        (out, info)
    };
    let (sealed, t1) = timed(|| sets.iter().map(seal).collect::<Vec<_>>());
    let (_, tn) = timed(|| parallel(&sets, threads, seal));
    report("seal: zstd-3 + pad + AES + id", plain, t1, tn, threads);
    let stored: usize = sealed.iter().map(|(s, _)| s.len()).sum();
    println!(
        "{:<34} {:.2}x smaller ({:.1} MB sealed)",
        "compression",
        plain as f64 / stored as f64,
        stored as f64 / 1e6
    );

    let open = |item: &(Vec<u8>, oxabl_cache_crypto::SegmentSeal)| {
        let (bytes, info) = item;
        let mut sink = std::io::sink();
        open_segment(
            &key,
            bytes,
            &SegmentExpectation {
                id: info.id,
                sealed_len: info.sealed_len,
                plain_len: info.plain_len,
                dictionary: None,
            },
            &limits,
            &mut sink,
        )
        .expect("open");
    };
    let (_, t1) = timed(|| sealed.iter().for_each(open));
    let (_, tn) = timed(|| parallel(&sealed, threads, open));
    report("open: id + AES + zstd decompress", plain, t1, tn, threads);

    let signer = Identity::generate().expect("identity");
    let ctx = |i: u32| RecordContext {
        cache_id: CacheId([1; 16]),
        epoch: 1,
        namespace: Namespace::Trusted,
        lookup_id: LookupId(*blake3::hash(&i.to_be_bytes()).as_bytes()),
    };
    let fields = EntryFields {
        segment_id: SegmentId([2; 32]),
        segment_len: 400_000,
        dict_id: None,
        plain_len: 2_000_000,
        created_at: 1_793_000_000,
        roster_version: 1,
        files: vec![
            FileEntry {
                path: "r/ar/customer.r".into(),
                size: 1_500_000,
            },
            FileEntry {
                path: "xref/ar/customer.xml".into(),
                size: 500_000,
            },
        ],
    };
    let count = 10_000u32;
    let record_key = root.record_key();
    let (records, t) = timed(|| {
        (0..count)
            .map(|i| seal_record(&record_key, &ctx(i), &fields, &signer, &limits).expect("seal"))
            .collect::<Vec<_>>()
    });
    println!(
        "{:<34} {:.1} us each",
        "record seal (sign + AES)",
        t / f64::from(count) * 1e6
    );
    let (_, t) = timed(|| {
        records.iter().enumerate().for_each(|(i, r)| {
            drop(open_record(&record_key, &ctx(i as u32), r, &limits).expect("open"))
        });
    });
    println!(
        "{:<34} {:.1} us each",
        "record open (AES + verify)",
        t / f64::from(count) * 1e6
    );

    let lookup = root.lookup_key();
    let (_, t) = timed(|| {
        (0..1_000_000u32).for_each(|i| {
            black_box(lookup.lookup_id(&[i as u8; 32]));
        })
    });
    println!(
        "{:<34} {:.3} us each",
        "lookup id (keyed BLAKE3)",
        t / 1e6 * 1e6
    );
}

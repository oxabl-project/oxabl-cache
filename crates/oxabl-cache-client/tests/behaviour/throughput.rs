//! Throughput of a gigabyte of synthetic r-code against the local server.
//!
//! Run in release mode, with the output shown:
//! `cargo test --release -p oxabl-cache-client --test behaviour -- --ignored --nocapture throughput`
//! `OXCACHE_THROUGHPUT_MB` sets the size in MiB (default 1024) and
//! `OXCACHE_THROUGHPUT_CONNECTIONS` the client's connections (default 32).

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use oxabl_cache_client::{
    ArtifactFileRef, ArtifactPath, Cache, CompileKey, Config, Credentials, Durability,
    FetchOptions, FetchOutcome, FetchRequest, IdentitySource, Mode, SecretString,
};
use oxabl_cache_crypto::{ExposeSecret, Identity};
use oxabl_cache_e2e::workdir::TestDir;
use oxabl_cache_format::Role;

use crate::support::fixture::rcode_like;
use crate::support::server::{Server, Timers};
use crate::support::setup::{Setup, genesis, member};
use crate::support::world::owner_key_text;

struct Corpus {
    keys: Vec<CompileKey>,
    refs: Vec<Vec<ArtifactFileRef>>,
    bytes: u64,
}

fn corpus(root: &Path, target: u64) -> Corpus {
    let mut corpus = Corpus {
        keys: Vec::new(),
        refs: Vec::new(),
        bytes: 0,
    };
    let mut n = 0u64;
    while corpus.bytes < target {
        let spread = (blake3::hash(&n.to_le_bytes()).as_bytes()[0] as f64) / 255.0;
        let size = (4_096.0 * (512.0f64).powf(spread)) as usize;
        let mut files = Vec::new();
        for (logical, len) in [("r", size), ("xref", size / 10 + 64)] {
            let name = format!("{logical}/unit{n}.{logical}");
            let bytes = rcode_like(&format!("throughput-{n}-{logical}"), len);
            let path = root.join(&name);
            fs::create_dir_all(path.parent().expect("parent")).expect("directory");
            fs::write(&path, &bytes).expect("write");
            corpus.bytes += bytes.len() as u64;
            files.push(ArtifactFileRef {
                path: ArtifactPath::new(&name).expect("path"),
                disk_path: path,
                size: bytes.len() as u64,
                blake3: *blake3::hash(&bytes).as_bytes(),
            });
        }
        corpus.keys.push(CompileKey(
            *blake3::hash(format!("key {n}").as_bytes()).as_bytes(),
        ));
        corpus.refs.push(files);
        n += 1;
    }
    corpus
}

fn peak_rss_mib() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|kb| kb.parse::<u64>().ok())
        })
        .map_or(0, |kb| kb / 1024)
}

fn cpu_seconds() -> f64 {
    let usage = nix::sys::resource::getrusage(nix::sys::resource::UsageWho::RUSAGE_SELF)
        .expect("getrusage");
    let seconds = |t: nix::sys::time::TimeVal| t.tv_sec() as f64 + t.tv_usec() as f64 / 1e6;
    seconds(usage.user_time()) + seconds(usage.system_time())
}

fn report(phase: &str, plain: u64, wire: u64, took: Duration, cpu: f64, requests: u64) {
    let secs = took.as_secs_f64();
    println!(
        "{phase:<26} {secs:>7.2} s   plain {:>7.1} MB/s   wire {:>7.1} MB/s   {requests} requests   client cpu {cpu:.1} s ({:.1} cores)",
        plain as f64 / 1e6 / secs,
        wire as f64 / 1e6 / secs,
        cpu / secs,
    );
}

fn config(repo: &Path, token: &str, identity: &str, state: &Path, mode: Mode) -> Config {
    Config::from_repo(repo)
        .expect("config")
        .with_mode(mode)
        .with_credentials(Credentials::Token(SecretString::from(token.to_owned())))
        .with_identity(IdentitySource::Value(SecretString::from(
            identity.to_owned(),
        )))
        .with_state_dir(state.to_path_buf())
        .with_concurrency(
            std::env::var("OXCACHE_THROUGHPUT_CONNECTIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(32),
        )
}

fn verify(stage: &Path, corpus: &Corpus) {
    for files in &corpus.refs {
        for file in files {
            let bytes = fs::read(stage.join(file.path.as_str())).expect("installed file");
            assert_eq!(
                blake3::hash(&bytes).as_bytes(),
                &file.blake3,
                "{}",
                file.path
            );
        }
    }
}

async fn fetch_all(
    cache: &Cache,
    corpus: &Corpus,
    stage: &Path,
    durability: Durability,
) -> Duration {
    let requests = corpus
        .keys
        .iter()
        .map(|&key| FetchRequest { key, expect: None })
        .collect();
    let mut options = FetchOptions::new(stage.to_path_buf());
    options.durability = durability;
    let started = Instant::now();
    let mut fetch = cache.fetch(requests, options);
    let mut hits = 0;
    let mut first = None;
    while let Some(outcome) = fetch.next().await {
        first.get_or_insert(started.elapsed());
        match outcome {
            FetchOutcome::Installed { .. } => hits += 1,
            miss @ FetchOutcome::Miss { .. } => panic!("{miss:?}"),
        }
    }
    if durability == Durability::Caller {
        let dir = fs::File::open(stage).expect("stage");
        nix::unistd::syncfs(&dir).expect("syncfs");
    }
    assert_eq!(hits, corpus.keys.len());
    println!(
        "  first outcome after {:.2} s",
        first.expect("an outcome").as_secs_f64()
    );
    started.elapsed()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "a measurement: run with --ignored --nocapture, in release mode"]
async fn throughput_of_synthetic_rcode_through_the_local_server() {
    let mebibytes: u64 = std::env::var("OXCACHE_THROUGHPUT_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024);
    let server = Server::start(None, &Timers::default());
    let setup = Setup::new(&server.address, &server.admin_address, &server.operator);
    let (tenant, admin) = setup.tenant("bench").await;
    let owner = Identity::generate().expect("identity");
    let (ci, dev) = (
        Identity::generate().expect("ci"),
        Identity::generate().expect("dev"),
    );
    let cache = setup
        .cache_with_quota(&admin, "bench", &owner.signing_public(), Some(64 << 30))
        .await;
    let ci_token = setup
        .token(&admin, "ci", &["read", "write:trusted"], &cache)
        .await;
    let dev_token = setup.token(&admin, "dev", &["read"], &cache).await;
    let owner_token = setup
        .token(&admin, "owner", &["read", "cache:admin"], &cache)
        .await;
    let (draft, roster) = genesis(
        &cache,
        vec![
            member("owner", Role::Owner, &owner),
            member("ci", Role::Ci, &ci),
            member("dev", Role::Developer, &dev),
        ],
        &owner,
    );
    setup.put_roster(&owner_token, &cache, 1, roster).await;

    let dirs = TestDir::new("throughput").expect("directory");
    let repo = dirs.path().join("repo");
    fs::create_dir_all(repo.join(".oxabl")).expect("directory");
    fs::write(
        repo.join(".oxabl/cache.toml"),
        format!(
            "server = \"http://{}\"\ntenant = \"{tenant}\"\ncache = \"{cache}\"\nowner_keys = [\"{}\"]\nmin_roster_version = 0\n",
            server.address,
            owner_key_text(&owner.signing_public()),
        ),
    )
    .expect("cache.toml");
    let started = Instant::now();
    let corpus = corpus(&dirs.path().join("workspace"), mebibytes << 20);
    println!(
        "generated {} sets, {:.1} MB in {:.1} s",
        corpus.keys.len(),
        corpus.bytes as f64 / 1e6,
        started.elapsed().as_secs_f64()
    );

    let ci_identity = ci.to_secret_string().expect("string");
    let writer = Cache::open(config(
        &repo,
        &ci_token,
        ci_identity.expose_secret(),
        &dirs.path().join("s1"),
        Mode::Ci,
    ))
    .await
    .expect("open");
    let (started, cpu_before) = (Instant::now(), cpu_seconds());
    let uploader = writer.uploader().expect("uploader");
    for (key, files) in corpus.keys.iter().zip(&corpus.refs) {
        uploader.put(*key, files.clone()).await;
    }
    let stored = uploader.finish().await;
    let took = started.elapsed();
    assert!(
        stored
            .outcomes
            .iter()
            .all(|(_, o)| *o == oxabl_cache_client::StoreOutcome::Committed),
        "{:?}",
        stored
            .outcomes
            .iter()
            .find(|(_, o)| *o != oxabl_cache_client::StoreOutcome::Committed)
    );
    println!();
    report(
        "store (ci)",
        corpus.bytes,
        stored.bytes_uploaded,
        took,
        cpu_seconds() - cpu_before,
        writer.stats().requests,
    );
    println!(
        "  compression: {:.2}x ({} MB sealed)",
        corpus.bytes as f64 / stored.bytes_uploaded as f64,
        stored.bytes_uploaded / 1_000_000
    );

    let dev_identity = dev.to_secret_string().expect("string");
    for (label, durability) in [
        ("fetch, durable batches", Durability::PerFile),
        ("fetch, caller syncs once", Durability::Caller),
    ] {
        let reader = Cache::open(config(
            &repo,
            &dev_token,
            dev_identity.expose_secret(),
            &dirs.path().join(label),
            Mode::Developer,
        ))
        .await
        .expect("open");
        let stage = dirs.path().join(format!("stage-{label}"));
        let cpu_before = cpu_seconds();
        let took = fetch_all(&reader, &corpus, &stage, durability).await;
        let stats = reader.stats();
        report(
            label,
            corpus.bytes,
            stats.bytes_received,
            took,
            cpu_seconds() - cpu_before,
            stats.requests,
        );
        verify(&stage, &corpus);
    }
    let (bytes, took) = raw_reads(
        &server,
        &cache,
        &dev_token,
        &draft.root_key().lookup_key(),
        &corpus,
    )
    .await;
    report("raw object reads", bytes, bytes, took, 0.0, 0);
    println!(
        "peak resident set of the test process: {} MiB",
        peak_rss_mib()
    );
}

/// Reads every pack the corpus lives in as ranges of 8 MiB, 32 at a time, with plain HTTP:
/// what the server can serve with no client pipeline in the way.
async fn raw_reads(
    server: &Server,
    cache: &str,
    token: &str,
    lookup_key: &oxabl_cache_crypto::LookupKey,
    corpus: &Corpus,
) -> (u64, Duration) {
    use oxabl_cache_format::{LookupFlags, LookupRequest, LookupResponse, Namespaces};
    let request = LookupRequest {
        flags: LookupFlags {
            records: true,
            urls: true,
        },
        namespaces: Namespaces {
            trusted: true,
            personal: false,
        },
        ids: corpus
            .keys
            .iter()
            .take(oxabl_cache_format::limits::LOOKUP_IDS)
            .map(|k| lookup_key.lookup_id(&k.0))
            .collect(),
    };
    let shape = request.shape().expect("shape");
    let http = reqwest::Client::new();
    let answer = http
        .post(format!(
            "http://{}/v1/caches/{cache}/lookup",
            server.address
        ))
        .bearer_auth(token)
        .header("content-type", "application/vnd.oxabl-cache.v1")
        .body(request.encode().expect("frame"))
        .send()
        .await
        .expect("lookup")
        .bytes()
        .await
        .expect("bytes");
    let response = LookupResponse::decode(&answer, &shape).expect("OXLR");
    let sizes: std::collections::HashMap<usize, u64> = response
        .hits
        .iter()
        .filter_map(|h| h.record.as_ref())
        .fold(Default::default(), |mut acc, r| {
            let end = acc.entry(r.pack_idx as usize).or_insert(0);
            *end = (*end).max(r.offset + u64::from(r.length));
            acc
        });
    let urls: Vec<(String, u64)> = response
        .packs
        .iter()
        .enumerate()
        .map(|(i, p)| (p.url.to_owned(), sizes.get(&i).copied().unwrap_or(0)))
        .collect();
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(32));
    let started = Instant::now();
    let mut reads = tokio::task::JoinSet::new();
    for (url, size) in urls {
        for start in (24..size).step_by(8 << 20) {
            let end = (start + (8 << 20)).min(size) - 1;
            let (http, gate, url) = (http.clone(), gate.clone(), url.clone());
            reads.spawn(async move {
                let _turn = gate.acquire().await;
                http.get(url)
                    .header("range", format!("bytes={start}-{end}"))
                    .send()
                    .await
                    .expect("get")
                    .bytes()
                    .await
                    .expect("body")
                    .len() as u64
            });
        }
    }
    let mut total = 0;
    while let Some(read) = reads.join_next().await {
        total += read.expect("read");
    }
    (total, started.elapsed())
}

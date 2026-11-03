//! The startup probe refuses a bucket that cannot be used, and a good bucket reports
//! its capabilities and is left clean.

use crate::support::fixture::hex;
use crate::support::frames;
use crate::support::objects::{Kind, block_on, s3_section};
use oxabl_cache_e2e::garage::Garage;
use oxabl_cache_e2e::ports::free_port;
use oxabl_cache_e2e::s3::S3Bucket;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_oxabl-cache-server");

fn good_bucket() -> S3Bucket {
    match crate::STORE {
        Kind::Garage => block_on(Garage::shared().new_bucket()).expect("bucket"),
        Kind::Versitygw => {
            block_on(oxabl_cache_e2e::versitygw::Versitygw::shared().new_bucket()).expect("bucket")
        }
        Kind::Fs => panic!("an S3 scenario ran against a local store"),
    }
}

fn workdir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

fn run(name: &str, bucket: &S3Bucket, command: &str) -> Output {
    let dir = workdir(name);
    let config = format!(
        "[server]\nlisten = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\n\
         metrics_listen = \"127.0.0.1:0\"\n[metadata]\nurl = \"sqlite://{dir}/meta.db\"\n\
         {store}[keys]\nurl_key_file = \"{dir}/url.key\"\n",
        dir = dir.display(),
        store = s3_section(&dir, bucket),
    );
    let path = dir.join("server.toml");
    fs::write(&path, config).expect("config");
    let output = Command::new(BIN)
        .arg("-c")
        .arg(&path)
        .arg(command)
        .output()
        .expect("run");
    let _ = fs::remove_dir_all(&dir);
    output
}

fn refused(name: &str, bucket: &S3Bucket, command: &str, expect: &str) {
    let output = run(name, bucket, command);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{name}: {command} started");
    assert!(stderr.contains(expect), "{name}: {stderr}");
    assert!(
        !stderr.contains(&bucket.secret_key) && !stderr.contains("X-Amz-Signature"),
        "{name}: the failure leaks a credential or a signed URL: {stderr}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("ready"),
        "{name}: a ready line was printed"
    );
}

#[test]
fn a_good_bucket_reports_its_capabilities_and_is_left_without_probe_objects() {
    let bucket = good_bucket();
    let output = run("s3-probe-good", &bucket, "check-store");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let capabilities: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("check-store prints JSON");
    eprintln!("probe of {:?}: {capabilities}", crate::STORE);
    assert_eq!(capabilities["kind"], "s3");
    assert!(capabilities["if_none_match"].is_boolean());
    assert!(capabilities["checksum_sha256"].is_boolean());
    let left = block_on(bucket.list("")).expect("list");
    assert!(left.is_empty(), "the probe removed what it wrote: {left:?}");
}

#[test]
fn the_probe_refuses_a_missing_bucket_a_wrong_secret_and_a_wrong_endpoint() {
    let good = good_bucket();
    let missing = S3Bucket {
        bucket: format!("e2e-{}", hex(&frames::random(8))),
        ..good.clone()
    };
    refused(
        "s3-probe-missing",
        &missing,
        "check-store",
        "store probe failed",
    );
    let wrong_secret = S3Bucket {
        secret_key: hex(&frames::random(20)),
        ..good.clone()
    };
    refused(
        "s3-probe-secret",
        &wrong_secret,
        "check-store",
        "store probe failed",
    );
    let wrong_endpoint = S3Bucket {
        endpoint: format!("http://127.0.0.1:{}", free_port())
            .parse()
            .expect("url"),
        ..good
    };
    refused(
        "s3-probe-endpoint",
        &wrong_endpoint,
        "check-store",
        "store probe failed",
    );
    refused(
        "s3-probe-serve",
        &wrong_endpoint,
        "serve",
        "store probe failed",
    );
}

#[test]
fn the_probe_refuses_a_key_without_write_permission() {
    if crate::STORE != Kind::Garage {
        eprintln!(
            "skipped: {:?} is started with its root credential only, so there is no key to restrict",
            crate::STORE
        );
        return;
    }
    let read_only = block_on(Garage::shared().new_read_only_bucket()).expect("bucket");
    refused(
        "s3-probe-readonly",
        &read_only,
        "check-store",
        "store probe failed",
    );
}

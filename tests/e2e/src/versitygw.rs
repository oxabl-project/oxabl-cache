//! A versitygw S3 gateway on a POSIX directory with a fresh bucket per test.

use std::fs;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use url::Url;

use crate::s3::S3Bucket;
use crate::supervise::Supervised;
use crate::workdir::TestDir;
use crate::{keeper, ports, random, wait};

const REGION: &str = "us-east-1";
const STARTUP: Duration = Duration::from_secs(30);

/// A running gateway with one root credential.
pub struct Versitygw {
    endpoint: Url,
    access_key: String,
    secret_key: String,
    _process: Supervised,
    _dir: TestDir,
}

impl Versitygw {
    /// The gateway shared by every test in this binary.
    pub fn shared() -> &'static Versitygw {
        static SHARED: OnceLock<Result<Versitygw, String>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                keeper::run("versitygw-shared", || wait::retry(3, Versitygw::launch))
                    .map_err(|error| format!("{error:#}"))
            })
            .as_ref()
            .unwrap_or_else(|error| panic!("versitygw failed to start: {error}"))
    }

    /// Starts a gateway whose buckets are directories under the test directory.
    pub fn launch() -> Result<Self> {
        let dir = TestDir::new("versitygw")?;
        let port = ports::free_port();
        let (access_key, secret_key) = (random::hex(10), random::hex(20));
        let root = dir.path().join("root");
        let sidecar = dir.path().join("sidecar");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(&sidecar)?;

        let mut command = Command::new("versitygw");
        command
            .args(["--access", &access_key, "--secret", &secret_key])
            .args(["--region", REGION])
            .args(["--port", &format!("127.0.0.1:{port}")])
            .arg("posix")
            .arg("--sidecar")
            .arg(&sidecar)
            .arg(&root);
        let mut process = Supervised::spawn("versitygw", command, dir.path())?;
        wait::for_port(&mut process, port, STARTUP)?;

        Ok(Self {
            endpoint: format!("http://127.0.0.1:{port}").parse()?,
            access_key,
            secret_key,
            _process: process,
            _dir: dir,
        })
    }

    /// Creates a bucket that only this call's caller knows about.
    pub async fn new_bucket(&self) -> Result<S3Bucket> {
        let bucket = S3Bucket {
            endpoint: self.endpoint.clone(),
            region: REGION.to_owned(),
            access_key: self.access_key.clone(),
            secret_key: self.secret_key.clone(),
            bucket: format!("e2e-{}", random::hex(8)),
        };
        bucket.create().await?;
        Ok(bucket)
    }
}

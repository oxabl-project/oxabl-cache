//! A single-node Garage S3 server with a fresh bucket per test.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use url::Url;

use crate::s3::S3Bucket;
use crate::supervise::Supervised;
use crate::workdir::TestDir;
use crate::{cli, keeper, ports, random, wait};

const REGION: &str = "garage";
const STARTUP: Duration = Duration::from_secs(30);

/// A running Garage node with one access key.
pub struct Garage {
    config: PathBuf,
    endpoint: Url,
    access_key: String,
    secret_key: String,
    _process: Supervised,
    _dir: TestDir,
}

impl Garage {
    /// The node shared by every test in this binary.
    pub fn shared() -> &'static Garage {
        static SHARED: OnceLock<Result<Garage, String>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                keeper::run("garage-shared", || wait::retry(3, Garage::launch))
                    .map_err(|error| format!("{error:#}"))
            })
            .as_ref()
            .unwrap_or_else(|error| panic!("garage failed to start: {error}"))
    }

    /// Starts a node, assigns its layout and creates its access key.
    pub fn launch() -> Result<Self> {
        let dir = TestDir::new("garage")?;
        let (rpc_port, s3_port, admin_port) =
            (ports::free_port(), ports::free_port(), ports::free_port());
        let data = dir.path().join("data");
        let meta = dir.path().join("meta");
        let config = dir.path().join("garage.toml");
        fs::write(
            &config,
            format!(
                "metadata_dir = \"{meta}\"\n\
                 data_dir = \"{data}\"\n\
                 db_engine = \"sqlite\"\n\
                 replication_factor = 1\n\
                 rpc_bind_addr = \"127.0.0.1:{rpc_port}\"\n\
                 rpc_public_addr = \"127.0.0.1:{rpc_port}\"\n\
                 rpc_secret = \"{secret}\"\n\n\
                 [s3_api]\n\
                 s3_region = \"{REGION}\"\n\
                 api_bind_addr = \"127.0.0.1:{s3_port}\"\n\n\
                 [admin]\n\
                 api_bind_addr = \"127.0.0.1:{admin_port}\"\n\
                 admin_token = \"{token}\"\n",
                meta = meta.display(),
                data = data.display(),
                secret = random::hex(32),
                token = random::hex(16),
            ),
        )?;

        let mut server = Command::new("garage");
        server.arg("-c").arg(&config).arg("server");
        let mut process = Supervised::spawn("garage", server, dir.path())?;
        wait::for_port(&mut process, s3_port, STARTUP)?;

        wait::until(STARTUP, || {
            process.ensure_running()?;
            garage_cli(&config, &["status"])
        })?;
        let node = garage_cli(&config, &["node", "id", "-q"])?;
        let node_id = node.trim().split('@').next().context("empty node id")?;
        garage_cli(
            &config,
            &["layout", "assign", "-z", "dc1", "-c", "1G", node_id],
        )?;
        garage_cli(&config, &["layout", "apply", "--version", "1"])?;
        let key = garage_cli(&config, &["key", "create", "e2e"])?;

        Ok(Self {
            endpoint: format!("http://127.0.0.1:{s3_port}").parse()?,
            access_key: field(&key, "Key ID")?,
            secret_key: field(&key, "Secret key")?,
            config,
            _process: process,
            _dir: dir,
        })
    }

    /// Creates a bucket and a second key that may only read it.
    pub async fn new_read_only_bucket(&self) -> Result<S3Bucket> {
        let bucket = self.new_bucket().await?;
        let key = garage_cli(
            &self.config,
            &["key", "create", &format!("e2e-ro-{}", random::hex(4))],
        )?;
        let (access_key, secret_key) = (field(&key, "Key ID")?, field(&key, "Secret key")?);
        let allow = [
            "bucket",
            "allow",
            "--read",
            &bucket.bucket,
            "--key",
            &access_key,
        ];
        garage_cli(&self.config, &allow)?;
        let bucket = S3Bucket {
            access_key,
            secret_key,
            ..bucket
        };
        until_answers(|| bucket.head("e2e-ready")).await?;
        Ok(bucket)
    }

    /// Creates a bucket that only this call's caller knows about.
    pub async fn new_bucket(&self) -> Result<S3Bucket> {
        let name = format!("e2e-{}", random::hex(8));
        garage_cli(&self.config, &["bucket", "create", &name])?;
        let allow = [
            "bucket",
            "allow",
            "--read",
            "--write",
            "--owner",
            &name,
            "--key",
            &self.access_key,
        ];
        garage_cli(&self.config, &allow)?;
        let bucket = S3Bucket {
            endpoint: self.endpoint.clone(),
            region: REGION.to_owned(),
            access_key: self.access_key.clone(),
            secret_key: self.secret_key.clone(),
            bucket: name,
        };
        until_answers(|| async {
            bucket.put("e2e-ready", b"ready".to_vec()).await?;
            bucket.delete("e2e-ready").await
        })
        .await?;
        Ok(bucket)
    }
}

/// Garage applies a new layout, bucket and key grant a little after its CLI returns, and
/// its health route reports ready before any layout exists; a request is the only proof.
async fn until_answers<T, F: Future<Output = Result<T>>>(
    mut request: impl FnMut() -> F,
) -> Result<T> {
    let deadline = std::time::Instant::now() + STARTUP;
    loop {
        match request().await {
            Ok(value) => return Ok(value),
            Err(error) if std::time::Instant::now() >= deadline => {
                return Err(error.context("garage never accepted the bucket"));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

fn garage_cli(config: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("garage");
    command.arg("-c").arg(config).args(args);
    cli::run(&mut command)
}

fn field(output: &str, label: &str) -> Result<String> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(label)?.strip_prefix(':'))
        .map(|value| value.trim().to_owned())
        .ok_or_else(|| anyhow!("no `{label}` in garage output:\n{output}"))
}

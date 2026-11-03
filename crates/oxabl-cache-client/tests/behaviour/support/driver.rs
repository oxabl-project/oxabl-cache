//! Running the client in a child process, for behaviour that depends on its environment.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;

static BINARY: OnceLock<PathBuf> = OnceLock::new();

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .components()
        .collect()
}

fn binary() -> &'static Path {
    BINARY.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let mut command = Command::new(cargo);
        command.current_dir(workspace_root()).args([
            "build",
            "-p",
            "oxabl-cache-client",
            "--example",
            "driver",
            "--locked",
            "--message-format=json",
        ]);
        if !cfg!(debug_assertions) {
            command.arg("--release");
        }
        let output = command.output().expect("run cargo build");
        assert!(
            output.status.success(),
            "building the driver failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == "driver")
            .find_map(|m| m["executable"].as_str().map(PathBuf::from))
            .expect("the driver executable")
    })
}

/// Opens the cache of `root` in a process whose whole environment is `env`; returns its
/// JSON answer.
pub async fn open(root: &Path, env: Vec<(String, String)>) -> Value {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let output = Command::new(binary())
            .args(["open"])
            .arg(&root)
            .env_clear()
            .envs(env)
            .output()
            .expect("run the driver");
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let line = text.lines().last().unwrap_or_else(|| {
            panic!(
                "no answer; stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        serde_json::from_str(line).expect("the driver prints JSON")
    })
    .await
    .expect("join")
}

/// A running `driver fetch`, which a test may kill.
pub struct Fetching {
    child: std::process::Child,
}

impl Fetching {
    pub fn start(root: &Path, stage: &Path, count: usize, env: Vec<(String, String)>) -> Self {
        let child = Command::new(binary())
            .arg("fetch")
            .arg(root)
            .arg(stage)
            .arg(count.to_string())
            .env_clear()
            .envs(env)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn the driver");
        Self { child }
    }

    /// SIGKILLs the process and waits for it.
    pub fn kill(mut self) {
        self.child.kill().expect("kill");
        let _ = self.child.wait();
    }
}

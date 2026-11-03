//! The real `oxabl-cache-server` binary on a fresh data directory, supervised by the harness.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use oxabl_cache_e2e::supervise::Supervised;
use oxabl_cache_e2e::workdir::TestDir;

static BINARY: OnceLock<PathBuf> = OnceLock::new();

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .components()
        .collect()
}

/// Builds the server once per test binary and returns the executable's path.
pub fn binary() -> &'static Path {
    BINARY.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let mut command = Command::new(cargo);
        command.current_dir(workspace_root()).args([
            "build",
            "-p",
            "oxabl-cache-server",
            "--locked",
            "--message-format=json",
        ]);
        if !cfg!(debug_assertions) {
            command.arg("--release");
        }
        let output = command.output().expect("run cargo build");
        assert!(
            output.status.success(),
            "building the server failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|message| message["reason"] == "compiler-artifact")
            .filter(|message| message["target"]["name"] == "oxabl-cache-server")
            .find_map(|message| message["executable"].as_str().map(PathBuf::from))
            .expect("the server executable")
    })
}

/// Timers a test may shorten; everything else is the server's own default.
pub struct Timers {
    pub grace: &'static str,
    pub lease: &'static str,
    pub interval: &'static str,
    pub url_lifetime: &'static str,
}

impl Default for Timers {
    fn default() -> Self {
        Self {
            grace: "61s",
            lease: "2s",
            interval: "200ms",
            url_lifetime: "60s",
        }
    }
}

pub struct Server {
    _process: Supervised,
    pub dir: TestDir,
    /// The address the server listens on.
    pub address: String,
    pub admin_address: String,
    pub operator: String,
}

fn config_text(dir: &Path, public_url: Option<&str>, timers: &Timers) -> String {
    let dir = dir.display();
    let public_url = public_url.map_or_else(String::new, |url| format!("public_url = \"{url}\"\n"));
    format!(
        "[server]\nlisten = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\n\
         metrics_listen = \"127.0.0.1:0\"\n{public_url}\
         url_lifetime = \"{}\"\nshutdown_timeout = \"1s\"\n\
         [metadata]\nurl = \"sqlite://{dir}/meta.db\"\n\
         [store]\nkind = \"fs\"\nroot = \"{dir}/objects\"\n\
         [keys]\nurl_key_file = \"{dir}/url.key\"\n\
         [gc]\ngrace = \"{}\"\nlease = \"{}\"\ninterval = \"{}\"\n",
        timers.url_lifetime, timers.grace, timers.lease, timers.interval,
    )
}

fn ready_line(log: &Path, deadline: Duration) -> serde_json::Value {
    let started = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(log)
            && let Some(line) = text.lines().find(|l| l.starts_with("{\"ready\""))
        {
            return serde_json::from_str(line).expect("ready line");
        }
        assert!(
            started.elapsed() < deadline,
            "the server never became ready"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A free loopback port, held until the listener is dropped.
pub fn reserve_port() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    (listener, port)
}

impl Server {
    /// Starts a server that names `public_url` in the object URLs it hands out, or its own
    /// address when there is none.
    pub fn start(public_url: Option<&str>, timers: &Timers) -> Self {
        let dir = TestDir::new("server").expect("test directory");
        let config = dir.path().join("server.toml");
        fs::write(&config, config_text(dir.path(), public_url, timers)).expect("config");
        let init = Command::new(binary())
            .arg("-c")
            .arg(&config)
            .arg("init")
            .output()
            .expect("init");
        assert!(
            init.status.success(),
            "init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        let operator = String::from_utf8(init.stdout)
            .expect("utf8")
            .trim()
            .to_owned();
        let mut serve = Command::new(binary());
        serve.arg("-c").arg(&config).arg("serve");
        let process = Supervised::spawn("server", serve, dir.path()).expect("spawn the server");
        let ready = ready_line(&dir.path().join("server.log"), Duration::from_secs(30));
        Self {
            address: ready["ready"]["listen"]
                .as_str()
                .expect("listen")
                .to_owned(),
            admin_address: ready["ready"]["admin_listen"]
                .as_str()
                .expect("admin")
                .to_owned(),
            _process: process,
            dir,
            operator,
        }
    }

    /// The pack object files on disk.
    pub fn pack_files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.dir.path().join("objects/v1"), &mut out);
        out
    }
}

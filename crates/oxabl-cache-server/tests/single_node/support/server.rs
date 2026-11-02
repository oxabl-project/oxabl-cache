//! The real `oxabl-cache-server` binary on a fresh data directory.

use super::http::{Response, request};
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct Options {
    pub grace: &'static str,
    pub lease: &'static str,
    pub interval: &'static str,
    pub url_lifetime: &'static str,
    pub shutdown_timeout: &'static str,
    pub extra: &'static str,
    pub tls: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            grace: "2s",
            lease: "2s",
            interval: "200ms",
            url_lifetime: "5s",
            shutdown_timeout: "1s",
            extra: "",
            tls: false,
        }
    }
}

pub struct Server {
    child: Option<Child>,
    pub public: String,
    pub admin: String,
    pub metrics: String,
    pub public_url: String,
    pub dir: PathBuf,
    pub operator: String,
    pub cert_pem: Option<String>,
    config: PathBuf,
    runs: u32,
}

const BIN: &str = env!("CARGO_BIN_EXE_oxabl-cache-server");

fn config_text(dir: &Path, options: &Options) -> String {
    let dir = dir.display();
    format!(
        "[server]\nlisten = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\n\
         metrics_listen = \"127.0.0.1:0\"\nurl_lifetime = \"{}\"\nshutdown_timeout = \"{}\"\n\
         [metadata]\nurl = \"sqlite://{dir}/meta.db\"\n\
         [store]\nkind = \"fs\"\nroot = \"{dir}/objects\"\n\
         [keys]\nurl_key_file = \"{dir}/url.key\"\n\
         [gc]\ngrace = \"{}\"\nlease = \"{}\"\ninterval = \"{}\"\n{}\n",
        options.url_lifetime,
        options.shutdown_timeout,
        options.grace,
        options.lease,
        options.interval,
        options.extra,
    ) + &if options.tls {
        format!("[tls]\ncert_file = \"{dir}/tls.crt\"\nkey_file = \"{dir}/tls.key\"\n")
    } else {
        String::new()
    }
}

impl Server {
    pub fn start(name: &str, options: Options) -> Self {
        let dir =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("data dir");
        let config = dir.join("server.toml");
        let mut cert_pem = None;
        if options.tls {
            let issued = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
                .expect("certificate");
            fs::write(dir.join("tls.crt"), issued.cert.pem()).expect("cert file");
            fs::write(dir.join("tls.key"), issued.signing_key.serialize_pem()).expect("key file");
            cert_pem = Some(issued.cert.pem());
        }
        fs::write(&config, config_text(&dir, &options)).expect("config");
        let init = Command::new(BIN)
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
        let mut server = Self {
            child: None,
            public: String::new(),
            admin: String::new(),
            metrics: String::new(),
            public_url: String::new(),
            dir,
            operator,
            cert_pem,
            config,
            runs: 0,
        };
        server.spawn();
        server
    }

    fn stderr_path(&self, run: u32) -> PathBuf {
        self.dir.join(format!("stderr-{run}.log"))
    }

    fn spawn(&mut self) {
        self.runs += 1;
        let stderr = File::create(self.stderr_path(self.runs)).expect("stderr file");
        let mut child = Command::new(BIN)
            .arg("-c")
            .arg(&self.config)
            .arg("serve")
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn server");
        let stdout = child.stdout.take().expect("stdout");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            if let Some(Ok(line)) = lines.next() {
                let _ = sender.send(line);
            }
            for _ in lines {}
        });
        let line = receiver
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("no ready line; stderr: {}", self.stderr()));
        let ready: Value = serde_json::from_str(&line).expect("ready line is JSON");
        let ready = &ready["ready"];
        self.public = ready["listen"].as_str().expect("listen").to_owned();
        self.admin = ready["admin_listen"].as_str().expect("admin").to_owned();
        self.metrics = ready["metrics_listen"]
            .as_str()
            .expect("metrics")
            .to_owned();
        self.public_url = ready["public_url"].as_str().expect("public_url").to_owned();
        self.child = Some(child);
    }

    pub fn pid(&self) -> u32 {
        self.child.as_ref().expect("running").id()
    }

    pub fn sigterm(&self) {
        let status = Command::new("kill")
            .args(["-TERM", &self.pid().to_string()])
            .status()
            .expect("kill");
        assert!(status.success());
    }

    pub fn wait_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let started = Instant::now();
        let child = self.child.as_mut()?;
        while started.elapsed() < timeout {
            if let Some(status) = child.try_wait().expect("wait") {
                self.child = None;
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    pub fn stop(&mut self) -> ExitStatus {
        self.sigterm();
        self.wait_exit(Duration::from_secs(20))
            .expect("the server exits after SIGTERM")
    }

    pub fn restart(&mut self) {
        if self.child.is_some() {
            assert!(self.stop().success(), "graceful stop");
        }
        self.spawn();
    }

    /// Everything the server wrote to stderr, over every run.
    pub fn stderr(&self) -> String {
        (1..=self.runs)
            .filter_map(|run| fs::read_to_string(self.stderr_path(run)).ok())
            .collect()
    }

    pub fn admin_call(&self, method: &str, path: &str, body: Option<&Value>) -> Response {
        call(&self.admin, &self.operator, method, path, body)
    }

    pub fn create_tenant(&self, name: &str, extra: Value) -> String {
        let mut body = json!({"name": name});
        if let (Some(fields), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
            fields.extend(extra.clone());
        }
        let created = self.admin_call("POST", "/admin/v1/tenants", Some(&body));
        assert_eq!(
            created.status,
            201,
            "{}",
            String::from_utf8_lossy(&created.body)
        );
        created.json()["id"].as_str().expect("id").to_owned()
    }

    pub fn tenant_admin(&self, tenant: &str) -> Client {
        let path = format!("/admin/v1/tenants/{tenant}/tokens");
        let created = self.admin_call("POST", &path, Some(&json!({"name": "admin"})));
        assert_eq!(
            created.status,
            201,
            "{}",
            String::from_utf8_lossy(&created.body)
        );
        Client {
            addr: self.public.clone(),
            token: created.json()["token"].as_str().expect("token").to_owned(),
        }
    }

    pub fn metrics_text(&self) -> String {
        let page = request(&self.metrics, "GET", "/metrics", &[], b"");
        assert_eq!(page.status, 200);
        String::from_utf8(page.body).expect("utf8")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if std::env::var_os("OXCACHE_KEEP_TEST_DIRS").is_none() {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

pub fn call(addr: &str, token: &str, method: &str, path: &str, body: Option<&Value>) -> Response {
    let bytes = body.map(Value::to_string).unwrap_or_default();
    let mut headers = vec![("Authorization", format!("Bearer {token}"))];
    if body.is_some() {
        headers.push(("Content-Type", "application/json".to_owned()));
    }
    request(addr, method, path, &headers, bytes.as_bytes())
}

/// A caller of the public API with a bearer token.
#[derive(Clone)]
pub struct Client {
    pub addr: String,
    pub token: String,
}

impl Client {
    pub fn json(&self, method: &str, path: &str, body: Option<&Value>) -> Response {
        call(&self.addr, &self.token, method, path, body)
    }

    pub fn binary(&self, method: &str, path: &str, body: &[u8]) -> Response {
        request(
            &self.addr,
            method,
            path,
            &[
                ("Authorization", format!("Bearer {}", self.token)),
                ("Content-Type", "application/vnd.oxabl-cache.v1".to_owned()),
            ],
            body,
        )
    }

    pub fn with_token(&self, token: &str) -> Self {
        Self {
            addr: self.addr.clone(),
            token: token.to_owned(),
        }
    }
}

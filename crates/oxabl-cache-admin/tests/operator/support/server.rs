//! The real `oxabl-cache-server` binary on a fresh directory, built on demand.

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Once;

static BUILD: Once = Once::new();

fn server_binary() -> PathBuf {
    let admin = Path::new(env!("CARGO_BIN_EXE_oxabl-cache-admin"));
    let profile_dir = admin.parent().expect("profile directory");
    BUILD.call_once(|| {
        let mut build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        build
            .args(["build", "--locked", "-p", "oxabl-cache-server"])
            .current_dir(env!("CARGO_MANIFEST_DIR"));
        if profile_dir
            .file_name()
            .is_some_and(|name| name == "release")
        {
            build.arg("--release");
        }
        assert!(
            build.status().expect("cargo build").success(),
            "server build"
        );
    });
    profile_dir.join("oxabl-cache-server")
}

pub struct Server {
    child: Option<Child>,
    pub public: String,
    pub admin: String,
    pub dir: PathBuf,
    pub operator_token_file: PathBuf,
    pub cert_pem: Option<String>,
    pub tls: bool,
}

impl Server {
    pub fn start(name: &str, tls: bool) -> Self {
        Self::start_with(name, tls, "")
    }

    /// Like [`Server::start`] with `extra` appended to the configuration.
    pub fn start_with(name: &str, tls: bool, extra: &str) -> Self {
        let bin = server_binary();
        let dir =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("data dir");
        let shown = dir.display();
        let mut config = format!(
            "[server]\nlisten = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\n\
             metrics_listen = \"127.0.0.1:0\"\nshutdown_timeout = \"1s\"\n\
             [metadata]\nurl = \"sqlite://{shown}/meta.db\"\n\
             [store]\nkind = \"fs\"\nroot = \"{shown}/objects\"\n\
             [keys]\nurl_key_file = \"{shown}/url.key\"\n"
        );
        let mut cert_pem = None;
        if tls {
            let issued = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
                .expect("certificate");
            fs::write(dir.join("tls.crt"), issued.cert.pem()).expect("cert");
            fs::write(dir.join("tls.key"), issued.signing_key.serialize_pem()).expect("key");
            cert_pem = Some(issued.cert.pem());
            config.push_str(&format!(
                "[tls]\ncert_file = \"{shown}/tls.crt\"\nkey_file = \"{shown}/tls.key\"\n"
            ));
        }
        config.push_str(extra);
        let config_path = dir.join("server.toml");
        fs::write(&config_path, config).expect("config");
        let init = Command::new(&bin)
            .arg("-c")
            .arg(&config_path)
            .arg("init")
            .output()
            .expect("init");
        assert!(
            init.status.success(),
            "init: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        let operator_token_file = dir.join("operator.token");
        fs::write(&operator_token_file, &init.stdout).expect("token file");
        fs::set_permissions(&operator_token_file, fs::Permissions::from_mode(0o600)).expect("mode");
        let mut child = Command::new(&bin)
            .arg("-c")
            .arg(&config_path)
            .arg("serve")
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn server");
        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("stdout"))
            .read_line(&mut line)
            .expect("ready line");
        let ready: serde_json::Value = serde_json::from_str(&line).expect("ready json");
        let addr = |key: &str| ready["ready"][key].as_str().expect("address").to_owned();
        Self {
            child: Some(child),
            public: addr("listen"),
            admin: addr("admin_listen"),
            dir,
            operator_token_file,
            cert_pem,
            tls,
        }
    }

    pub fn operator_token(&self) -> String {
        fs::read_to_string(&self.operator_token_file)
            .expect("token")
            .trim()
            .to_owned()
    }

    fn scheme(&self) -> &'static str {
        if self.tls { "https" } else { "http" }
    }

    pub fn admin_url(&self) -> String {
        format!(
            "{}://{}",
            self.scheme(),
            self.admin.replace("127.0.0.1", "localhost")
        )
    }

    pub fn public_url(&self) -> String {
        format!(
            "{}://{}",
            self.scheme(),
            self.public.replace("127.0.0.1", "localhost")
        )
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status();
            let _ = child.wait();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

//! A private Secret Service: a D-Bus session bus on a socket in a test directory and an
//! unlocked `gnome-keyring-daemon` on it, so a child given [`SecretService::environment`] stores
//! and reads secrets without touching the user's keyring or asking anyone. The socket is a file,
//! so a child in a network namespace of its own reaches it too.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::supervise::Supervised;
use crate::wait;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// The bus and the keyring daemon, stopped on drop.
pub struct SecretService {
    _keyring: Supervised,
    _bus: Supervised,
    address: String,
    runtime: PathBuf,
}

fn short_dir(prefix: &str) -> Result<PathBuf> {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/e2e-tmp")
        .components()
        .collect();
    let dir = root.join(format!("{prefix}{n}-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    Ok(fs::canonicalize(dir)?)
}

impl SecretService {
    pub fn start() -> Result<Self> {
        let dir = short_dir("ss")?;
        let home = dir.join("home");
        let runtime = dir.join("run");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&runtime)?;
        fs::set_permissions(
            &runtime,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )?;
        let address = format!("unix:path={}", dir.join("bus").display());
        let config = dir.join("bus.conf");
        fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>{address}</listen><auth>EXTERNAL</auth>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n"
            ),
        )?;
        let mut bus = Command::new("dbus-daemon");
        bus.arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--nopidfile"]);
        let mut bus = Supervised::spawn("dbus", bus, &dir)?;
        wait::until(Duration::from_secs(10), || {
            bus.ensure_running()?;
            let probe = Command::new("dbus-send")
                .args([
                    "--session",
                    "--dest=org.freedesktop.DBus",
                    "--type=method_call",
                    "--print-reply",
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus.ListNames",
                ])
                .env("DBUS_SESSION_BUS_ADDRESS", &address)
                .output()?;
            anyhow::ensure!(probe.status.success(), "the bus is not up yet");
            Ok(())
        })
        .context("the session bus starts")?;
        let mut keyring = Command::new("sh");
        keyring
            .args([
                "-c",
                "printf test | exec gnome-keyring-daemon --unlock --foreground --components=secrets",
            ])
            .env("DBUS_SESSION_BUS_ADDRESS", &address)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_RUNTIME_DIR", &runtime);
        let mut keyring = Supervised::spawn("keyring", keyring, &dir)?;
        wait::until(Duration::from_secs(10), || {
            keyring.ensure_running()?;
            let probe = Command::new("dbus-send")
                .args([
                    "--session",
                    "--dest=org.freedesktop.secrets",
                    "--type=method_call",
                    "--print-reply",
                    "/org/freedesktop/secrets",
                    "org.freedesktop.DBus.Peer.Ping",
                ])
                .env("DBUS_SESSION_BUS_ADDRESS", &address)
                .output()?;
            anyhow::ensure!(probe.status.success(), "the keyring is not on the bus yet");
            Ok(())
        })
        .context("the keyring daemon answers")?;
        Ok(Self {
            _keyring: keyring,
            _bus: bus,
            address,
            runtime,
        })
    }

    /// The variables a child needs to use this Secret Service.
    pub fn environment(&self) -> Vec<(String, String)> {
        vec![
            ("DBUS_SESSION_BUS_ADDRESS".into(), self.address.clone()),
            ("XDG_RUNTIME_DIR".into(), self.runtime.display().to_string()),
        ]
    }
}

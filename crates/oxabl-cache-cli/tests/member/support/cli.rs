//! Runs the `oxabl-cache` binary the way a person's shell would, without a terminal.

use std::path::Path;
use std::process::{Command, Stdio};

use oxabl_cache_e2e::session::without_terminal;
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_oxabl-cache");

/// What a run printed and how it ended.
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim())
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{self}"))
    }

    /// Panics with everything printed unless the run exited 0.
    pub fn ok(self) -> Self {
        assert_eq!(self.code, 0, "expected success:\n{self}");
        self
    }

    /// Panics with everything printed unless the run exited with `code`.
    pub fn exits(self, code: i32) -> Self {
        assert_eq!(self.code, code, "expected exit {code}:\n{self}");
        self
    }

    pub fn everything(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

impl std::fmt::Display for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit {}\nstdout:\n{}\nstderr:\n{}",
            self.code, self.stdout, self.stderr
        )
    }
}

/// Runs the binary with `home` as the person's home and no `OXABL_CACHE_*` or `CI` variable.
pub fn run(home: &Path, args: &[String], extra_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
        .stdin(Stdio::null());
    for (name, _) in std::env::vars() {
        if name.starts_with("OXABL_CACHE_") || name == "CI" {
            command.env_remove(name);
        }
    }
    for (name, value) in extra_env {
        command.env(name, value);
    }
    without_terminal(&mut command);
    let output = command.output().expect("run oxabl-cache");
    Output {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Runs the binary on a pseudo-terminal and types `answers` at its prompts, one per prompt.
/// Returns the exit code and everything the terminal showed, which includes any echo.
pub fn run_on_terminal(home: &Path, args: &[String], answers: &[&str]) -> (i32, String) {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use oxabl_cache_e2e::session::Terminal;

    let terminal = Terminal::open().expect("pseudo-terminal");
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"));
    for (name, _) in std::env::vars() {
        if name.starts_with("OXABL_CACHE_") || name == "CI" {
            command.env_remove(name);
        }
    }
    terminal.attach(&mut command).expect("attach");
    let mut child = command.spawn().expect("run oxabl-cache");
    drop(command);
    let mut master = terminal.release_slave();
    let shown = Arc::new(Mutex::new(Vec::<u8>::new()));
    let reader = {
        let (shown, mut master) = (shown.clone(), master.try_clone().expect("master"));
        std::thread::spawn(move || {
            let mut chunk = [0u8; 1024];
            while let Ok(n) = master.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                shown
                    .lock()
                    .expect("transcript")
                    .extend_from_slice(&chunk[..n]);
            }
        })
    };
    let mut consumed = 0;
    for answer in answers {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            {
                let seen = shown.lock().expect("transcript");
                if seen.len() > consumed && seen.ends_with(b": ") {
                    consumed = seen.len();
                    break;
                }
            }
            assert!(Instant::now() < deadline, "no prompt appeared");
            std::thread::sleep(Duration::from_millis(20));
        }
        master
            .write_all(format!("{answer}\n").as_bytes())
            .expect("type");
    }
    let code = child.wait().expect("wait").code().unwrap_or(-1);
    let _ = reader.join();
    let transcript = String::from_utf8_lossy(&shown.lock().expect("transcript")).into_owned();
    (code, transcript)
}

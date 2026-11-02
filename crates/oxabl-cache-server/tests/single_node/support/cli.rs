//! Running the binary's other subcommands against an arbitrary configuration.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_oxabl-cache-server");

/// Writes `config` into a fresh directory and runs the binary with `args` and `env`.
pub fn run(name: &str, config: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("server.toml");
    fs::write(&path, config.replace("{dir}", &dir.display().to_string())).expect("config");
    let output = Command::new(BIN)
        .arg("-c")
        .arg(&path)
        .args(args)
        .envs(env.iter().copied())
        .output()
        .expect("run");
    let _ = fs::remove_dir_all(&dir);
    output
}

//! The real `oxabl-cache-admin` binary.

use super::server::Server;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_oxabl-cache-admin");

pub struct Run {
    pub output: Output,
}

impl Run {
    pub fn code(&self) -> i32 {
        self.output.status.code().expect("exit code")
    }

    pub fn stdout(&self) -> String {
        String::from_utf8(self.output.stdout.clone()).expect("utf8")
    }

    pub fn stderr(&self) -> String {
        String::from_utf8(self.output.stderr.clone()).expect("utf8")
    }

    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout())
            .unwrap_or_else(|e| panic!("json {e}: {}", self.stdout()))
    }
}

/// Runs the binary against a server's admin listener with the operator token file.
pub fn run(server: &Server, args: &[&str]) -> Run {
    run_with(server, &server.operator_token_file, None, args)
}

pub fn run_with(server: &Server, token_file: &Path, ca_file: Option<&Path>, args: &[&str]) -> Run {
    let mut command = Command::new(BIN);
    command
        .args(["--server", &server.admin_url(), "--token-file"])
        .arg(token_file);
    if let Some(ca) = ca_file {
        command.arg("--ca-file").arg(ca);
    }
    command.args(args);
    Run {
        output: command.output().expect("run admin"),
    }
}

/// Runs the binary with no connection settings at all.
pub fn bare(args: &[&str], env: &[(&str, &str)]) -> Run {
    Run {
        output: Command::new(BIN)
            .args(args)
            .env_remove("OXABL_CACHE_ADMIN_SERVER")
            .env_remove("OXABL_CACHE_ADMIN_TOKEN_FILE")
            .envs(env.iter().copied())
            .output()
            .expect("run admin"),
    }
}

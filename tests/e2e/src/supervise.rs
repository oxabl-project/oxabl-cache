//! Supervised child processes: own process group, die with the parent, killed on drop.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

use crate::pdeathsig::die_with_parent;

/// A child process whose stdout and stderr go to `<dir>/<name>.log`.
pub struct Supervised {
    name: String,
    child: Child,
    log: PathBuf,
}

impl Supervised {
    pub fn spawn(name: &str, mut command: Command, dir: &Path) -> Result<Self> {
        let log = dir.join(format!("{name}.log"));
        let out = File::create(&log).with_context(|| format!("create {}", log.display()))?;
        command
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out);
        die_with_parent(&mut command);
        let child = command.spawn().with_context(|| format!("spawn {name}"))?;
        Ok(Self {
            name: name.to_owned(),
            child,
            log,
        })
    }

    /// Fails with the log tail if the process has exited.
    pub fn ensure_running(&mut self) -> Result<()> {
        if let Some(status) = self.child.try_wait()? {
            bail!("{} exited with {status}:\n{}", self.name, self.log_tail());
        }
        Ok(())
    }

    pub fn log_tail(&self) -> String {
        let log = std::fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<&str> = log.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

impl Drop for Supervised {
    fn drop(&mut self) {
        let _ = killpg(Pid::from_raw(self.child.id() as i32), Signal::SIGKILL);
        let _ = self.child.wait();
    }
}

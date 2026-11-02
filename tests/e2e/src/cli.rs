//! Short-lived command-line helpers.

use std::process::Command;

use anyhow::{Context, Result, bail};

/// Runs `command` to completion and returns its stdout; fails with its stderr on a non-zero exit.
pub fn run(command: &mut Command) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("run {command:?}"))?;
    if !output.status.success() {
        bail!(
            "{command:?} exited with {}:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

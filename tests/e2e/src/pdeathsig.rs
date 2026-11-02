//! The one place the harness needs `unsafe`: arranging for a child to die with its parent.

#![allow(unsafe_code)]

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::sys::prctl;
use nix::sys::signal::Signal;

/// Makes `command` spawn into its own process group and receive SIGKILL when the spawning
/// thread exits. The `pre_exec` closure only calls `prctl`, which is async-signal-safe.
pub fn die_with_parent(command: &mut Command) {
    command.process_group(0);
    unsafe {
        command.pre_exec(|| prctl::set_pdeathsig(Signal::SIGKILL).map_err(io::Error::from));
    }
}

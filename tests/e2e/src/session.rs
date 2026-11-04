//! Running a child with no controlling terminal, so that a prompt on `/dev/tty` fails instead
//! of waiting for whoever runs the tests, or with a pseudo-terminal of the test's own.

#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

nix::ioctl_write_int_bad!(take_controlling_terminal, nix::libc::TIOCSCTTY);

/// Starts `command` in a session of its own, which has no controlling terminal. The
/// `pre_exec` closure only calls `setsid`, which is async-signal-safe.
pub fn without_terminal(command: &mut Command) {
    unsafe {
        command.pre_exec(|| nix::unistd::setsid().map(|_| ()).map_err(io::Error::from));
    }
}

/// The master side of a pseudo-terminal, and the slave a child takes as its terminal.
pub struct Terminal {
    pub master: File,
    slave: OwnedFd,
}

impl Terminal {
    pub fn open() -> io::Result<Self> {
        let pty = nix::pty::openpty(None, None).map_err(io::Error::from)?;
        Ok(Self {
            master: File::from(pty.master),
            slave: pty.slave,
        })
    }

    /// Makes the slave the standard streams and the controlling terminal of `command`, in a
    /// session of its own. The `pre_exec` closure calls `setsid` and one `ioctl`, both
    /// async-signal-safe.
    pub fn attach(&self, command: &mut Command) -> io::Result<()> {
        command
            .stdin(Stdio::from(self.slave.try_clone()?))
            .stdout(Stdio::from(self.slave.try_clone()?))
            .stderr(Stdio::from(self.slave.try_clone()?));
        unsafe {
            command.pre_exec(|| {
                nix::unistd::setsid().map_err(io::Error::from)?;
                take_controlling_terminal(0, 0)
                    .map(|_| ())
                    .map_err(io::Error::from)
            });
        }
        Ok(())
    }

    /// Closes the parent's copy of the slave, so that reading the master ends when the child
    /// does.
    pub fn release_slave(self) -> File {
        drop(self.slave);
        self.master
    }
}

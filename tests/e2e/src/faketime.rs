//! The libfaketime preload library of the devshell, to run a server under a clock of the test's choosing.

use std::path::PathBuf;
use std::process::Command;

/// A specification that freezes the clock at the present instant, so that signatures checked by
/// another process (an S3 server) stay inside its clock-skew window.
pub fn frozen_now() -> String {
    let output = Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S")
        .output()
        .expect("run date");
    assert!(output.status.success(), "date failed");
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
}

/// Environment variables that run a program under `spec` (a libfaketime time specification:
/// a plain timestamp freezes the clock there, including the monotonic clock).
pub fn environment(spec: &str) -> Vec<(&'static str, String)> {
    let library = library().display().to_string();
    vec![
        ("LD_PRELOAD", library),
        ("FAKETIME", spec.to_owned()),
        ("FAKETIME_NO_CACHE", "1".to_owned()),
    ]
}

/// The multi-threaded libfaketime library next to the `faketime` binary on `PATH`.
pub fn library() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let binary = std::env::split_paths(&path)
        .map(|dir| dir.join("faketime"))
        .find(|candidate| candidate.is_file())
        .expect("libfaketime is on PATH in the devshell (nix develop)");
    let root = binary
        .canonicalize()
        .expect("resolve faketime")
        .parent()
        .and_then(|bin| bin.parent())
        .expect("faketime lives in a bin directory")
        .to_owned();
    let library = root.join("lib/libfaketimeMT.so.1");
    assert!(library.is_file(), "{} is missing", library.display());
    library
}

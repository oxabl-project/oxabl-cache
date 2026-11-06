//! Runs the current test again in a child process with extra environment variables, for code
//! under test that reads its environment (a session bus address) and a test that cannot set its
//! own environment safely.

use std::process::Command;
use std::thread;

const MARKER: &str = "OXABL_E2E_REEXEC";

/// Whether this process is the re-run.
pub fn inside() -> bool {
    std::env::var_os(MARKER).is_some()
}

/// Call first in a test. In the re-run it returns `true` and the test goes on. Otherwise it runs
/// the same test binary and test with `environment` added, asserts that run passed, and returns
/// `false`.
pub fn with_environment(environment: &[(String, String)]) -> bool {
    if inside() {
        return true;
    }
    let test = thread::current()
        .name()
        .expect("a named test thread")
        .to_owned();
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args([&test, "--exact", "--nocapture", "--test-threads=1"])
        .envs(environment.iter().map(|(k, v)| (k, v)))
        .env(MARKER, "1")
        .output()
        .expect("run the test again");
    assert!(
        output.status.success(),
        "{test} failed in its re-run:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

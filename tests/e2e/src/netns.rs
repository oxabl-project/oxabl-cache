//! Runs a test inside a user and network namespace of its own. The namespace is
//! configured as its root, then the test runs as the invoking user's own uid, so
//! programs that refuse to run as root (Postgres) work inside it.

use std::process::Command;
use std::thread;

const MARKER: &str = "OXABL_E2E_NETNS";

/// Whether this process already runs inside its namespace.
pub fn inside() -> bool {
    std::env::var_os(MARKER).is_some()
}

/// Call first in a test. Inside the namespace it returns `true` and the test goes on.
/// Outside, it re-runs the same test binary and test in a fresh namespace whose loopback
/// also answers on `addresses`, asserts that run passed, and returns `false`. The server
/// binary is built first, outside the namespace.
pub fn enter(addresses: &[&str]) -> bool {
    enter_with(addresses, &[])
}

/// [`enter`], with `environment` added to the test's run inside the namespace.
pub fn enter_with(addresses: &[&str], environment: &[(String, String)]) -> bool {
    if inside() {
        return true;
    }
    crate::cache_server::ensure_built();
    let test = thread::current()
        .name()
        .expect("a named test thread")
        .to_owned();
    let executable = std::env::current_exe().expect("test executable");
    let adds: String = addresses
        .iter()
        .map(|address| format!("ip addr add {address}/32 dev lo && "))
        .collect();
    let uid = nix::unistd::getuid();
    let script = format!(
        "ip link set lo up && {adds}sysctl -w net.ipv4.ip_unprivileged_port_start=0 >/dev/null \
         && exec unshare --user --map-user={uid} \"$0\" \"$@\""
    );
    let output = Command::new("unshare")
        .args(["--user", "--map-root-user", "--net", "sh", "-c", &script])
        .arg(executable)
        .args([&test, "--exact", "--nocapture", "--test-threads=1"])
        .envs(environment.iter().map(|(k, v)| (k, v)))
        .env(MARKER, "1")
        .output()
        .expect("run unshare");
    assert!(
        output.status.success(),
        "{test} failed inside its network namespace:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

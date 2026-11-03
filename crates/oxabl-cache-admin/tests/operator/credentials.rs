//! Operator credentials, an absent server and the secrecy of the token.

use crate::support::admin::{run, run_with};
use crate::support::server::Server;
use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn a_wrong_operator_token_is_refused_with_exit_one_and_the_envelope() {
    let server = Server::start("op-wrong", false);
    let operator = server.operator_token();
    let wrong = server.dir.join("wrong.token");
    let (stem, last) = operator.split_at(operator.len() - 1);
    let other = if last == "x" { "y" } else { "x" };
    fs::write(&wrong, format!("{stem}{other}")).expect("token");
    let refused = run_with(&server, &wrong, None, &["tenant", "list"]);
    assert_eq!(refused.code(), 1);
    assert!(
        refused.stderr().contains("unauthenticated"),
        "{}",
        refused.stderr()
    );
    assert_eq!(refused.stdout(), "");
    let json = run_with(&server, &wrong, None, &["--json", "health"]);
    assert_eq!(json.code(), 1);
    let empty = server.dir.join("empty.token");
    fs::write(&empty, "\n").expect("token");
    assert_eq!(run_with(&server, &empty, None, &["health"]).code(), 2);
}

#[test]
fn a_stopped_server_is_exit_three() {
    let mut server = Server::start("op-stopped", false);
    assert_eq!(run(&server, &["health"]).code(), 0);
    server.stop();
    let down = run(&server, &["health"]);
    assert_eq!(down.code(), 3, "{}", down.stderr());
    assert!(down.stderr().contains("unreachable"), "{}", down.stderr());
    assert_eq!(run(&server, &["--json", "tenant", "list"]).code(), 3);
}

#[test]
fn the_operator_token_is_in_no_output_no_argument_and_no_environment() {
    let mut server = Server::start("op-secret", false);
    let token = server.operator_token();
    let mut seen = String::new();
    for args in [
        &["tenant", "list"][..],
        &["--json", "tenant", "create", "acme"],
        &["health"],
        &["usage", "export", "--month", "2026-01"],
        &["tenant", "show", "nobody"],
        &["tenant", "create", "x", "--hard-quota", "lots"],
    ] {
        let result = run(&server, args);
        seen.push_str(&result.stdout());
        seen.push_str(&result.stderr());
    }
    let wrong = server.dir.join("wrong.token");
    fs::write(&wrong, "oxo_wrong_secret\n").expect("token");
    let refused = run_with(&server, &wrong, None, &["health"]);
    seen.push_str(&refused.stdout());
    seen.push_str(&refused.stderr());
    assert!(
        !seen.contains(&token),
        "the operator token reached an output"
    );
    assert!(
        !seen.contains("oxo_wrong_secret"),
        "a refused token reached an output"
    );

    let silent = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", silent.local_addr().expect("addr"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_oxabl-cache-admin"))
        .args(["--server", &url, "--token-file"])
        .arg(&server.operator_token_file)
        .arg("health")
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let _held = silent.accept().expect("the client connects");
    std::thread::sleep(Duration::from_millis(300));
    let pid = child.id();
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).expect("cmdline");
    let environ = fs::read(format!("/proc/{pid}/environ")).expect("environ");
    let found = |bytes: &[u8]| String::from_utf8_lossy(bytes).contains(&token);
    assert!(!found(&cmdline), "the token is on the command line");
    assert!(!found(&environ), "the token is in the environment");
    child.kill().expect("kill");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("read");
    let _ = child.wait();
    assert!(!stderr.contains(&token));
    server.stop();
}

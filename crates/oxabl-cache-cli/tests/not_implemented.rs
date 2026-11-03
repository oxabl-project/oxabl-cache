//! The member binary refuses every invocation until it is built.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_oxabl-cache");

#[test]
fn every_invocation_fails_with_one_line_saying_it_is_not_implemented() {
    let invocations: &[&[&str]] = &[
        &[],
        &["--help"],
        &["-h"],
        &["--version"],
        &["help"],
        &["login"],
        &["cache", "list", "--json"],
    ];
    for args in invocations {
        let output = Command::new(BIN).args(*args).output().expect("run");
        assert_ne!(output.status.code(), Some(0), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8(output.stderr).expect("utf8");
        assert_eq!(stderr.lines().count(), 1, "{args:?}: {stderr}");
        assert!(stderr.contains("not implemented yet"), "{args:?}: {stderr}");
    }
}

//! Which permission bits on a secret file make the server warn, and which files count.

use crate::STORE;
use crate::support::fixture::World;
use crate::support::objects::Kind;
use crate::support::server::Options;
use std::fs;
use std::os::unix::fs::PermissionsExt;

const WARNING: &str = "readable by other users";

/// The store's own secret: the URL key of a local-disk store, the secret key of an S3 store.
fn store_secret() -> &'static str {
    match STORE {
        Kind::Fs => "url.key",
        Kind::Garage | Kind::Versitygw => "s3-secret",
    }
}

/// The server's stderr after it restarts with `file` set to `mode`.
fn stderr_with_mode(name: &str, options: Options, file: &str, mode: u32) -> String {
    let mut world = World::start(name, options);
    let path = world.server.dir.join(file);
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("mode");
    world.server.restart();
    world.server.stderr()
}

fn warns_with_mode(name: &str, mode: u32) -> bool {
    stderr_with_mode(name, Options::default(), store_secret(), mode).contains(WARNING)
}

#[test]
fn a_private_key_file_does_not_warn() {
    assert!(!warns_with_mode("secret-mode-0600", 0o600));
}

#[test]
fn a_key_file_the_processs_own_group_reads_does_not_warn() {
    assert!(!warns_with_mode("secret-mode-0440", 0o440));
}

#[test]
fn a_key_file_others_or_a_group_writer_can_reach_warns() {
    for (name, mode) in [
        ("secret-mode-0444", 0o444),
        ("secret-mode-0404", 0o404),
        ("secret-mode-0460", 0o460),
        ("secret-mode-0644", 0o644),
    ] {
        assert!(warns_with_mode(name, mode), "{mode:o} warns");
    }
}

#[test]
fn a_claim_key_others_can_read_warns_and_names_the_file() {
    let options = Options {
        claim_key: true,
        ..Options::default()
    };
    let stderr = stderr_with_mode("secret-mode-claim", options, "claim.key", 0o644);
    assert!(
        stderr.contains(WARNING) && stderr.contains("claim.key"),
        "{stderr}"
    );
}

#[test]
fn a_url_key_only_a_local_disk_store_reads_warns_only_there() {
    let stderr = stderr_with_mode("secret-mode-url", Options::default(), "url.key", 0o644);
    assert_eq!(stderr.contains(WARNING), STORE == Kind::Fs, "{stderr}");
}

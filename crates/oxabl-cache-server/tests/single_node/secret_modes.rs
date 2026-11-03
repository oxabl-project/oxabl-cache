//! Which permission bits on the URL key file make the server warn.

use crate::support::fixture::World;
use crate::support::server::Options;
use std::fs;
use std::os::unix::fs::PermissionsExt;

const WARNING: &str = "readable by other users";

fn warns_with_mode(name: &str, mode: u32) -> bool {
    let mut world = World::start(name, Options::default());
    let key = world.server.dir.join("url.key");
    fs::set_permissions(&key, fs::Permissions::from_mode(mode)).expect("mode");
    world.server.restart();
    world.server.stderr().contains(WARNING)
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

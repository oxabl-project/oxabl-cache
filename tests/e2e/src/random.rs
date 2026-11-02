//! Random identifiers and secrets for test resources.

use std::fs::File;
use std::io::Read;

/// Returns `bytes` random bytes as lowercase hex.
pub fn hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    File::open("/dev/urandom")
        .and_then(|mut urandom| urandom.read_exact(&mut buf))
        .expect("read /dev/urandom");
    buf.iter().map(|byte| format!("{byte:02x}")).collect()
}

//! Sealing and opening a very large artifact set stays within a fixed address-space limit,
//! so no step holds a second full copy of the plaintext.

use std::io::{Read, Write};
use std::process::Command;

use nix::sys::resource::{Resource, setrlimit};
use oxabl_cache_crypto::{RootKey, SealOptions, SegmentExpectation, open_segment, seal_segment};
use oxabl_cache_format::Limits;

const PLAIN_BYTES: u64 = 400 * 1024 * 1024;
const LIMIT_BYTES: u64 = 320 * 1024 * 1024;
const MARK: &str = "OXC_MEMORY_CHILD";

struct Stream {
    left: u64,
    block: Vec<u8>,
    counter: u64,
    offset: usize,
}

impl Stream {
    fn new(total: u64) -> Self {
        let mut state = 0x1234_5678_9abc_def0u64;
        let block = (0..8192)
            .map(|i| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                if i % 5 == 0 {
                    (state >> 56) as u8
                } else {
                    b"CHARACTER INTEGER ./ar/x.p"[i % 25]
                }
            })
            .collect();
        Self {
            left: total,
            block,
            counter: 0,
            offset: 0,
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf
            .len()
            .min(self.left as usize)
            .min(self.block.len() - self.offset);
        buf[..n].copy_from_slice(&self.block[self.offset..self.offset + n]);
        if self.offset == 0 && n >= 8 {
            buf[..8].copy_from_slice(&self.counter.to_be_bytes());
        }
        self.offset += n;
        if self.offset == self.block.len() {
            self.offset = 0;
            self.counter += 1;
        }
        self.left -= n as u64;
        Ok(n)
    }
}

struct Digest(blake3::Hasher, u64);

impl Write for Digest {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        self.1 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn digest_of_input() -> blake3::Hash {
    let mut sink = Digest(blake3::Hasher::new(), 0);
    std::io::copy(&mut Stream::new(PLAIN_BYTES), &mut sink).expect("copy");
    sink.0.finalize()
}

fn child_seal_and_open() {
    let expected = digest_of_input();
    setrlimit(Resource::RLIMIT_AS, LIMIT_BYTES, LIMIT_BYTES).expect("setrlimit");

    let root = RootKey::from_bytes([9; 32]);
    let limits = Limits::new(1024 * 1024 * 1024).expect("limits");
    let mut sealed = Vec::new();
    let info = seal_segment(
        &root.segment_key(),
        Stream::new(PLAIN_BYTES),
        &SealOptions::default(),
        &limits,
        &mut sealed,
    )
    .expect("seal under the address-space limit");
    assert_eq!(info.plain_len, PLAIN_BYTES);
    assert!(
        sealed.len() < 32 * 1024 * 1024,
        "{} sealed bytes",
        sealed.len()
    );

    let mut sink = Digest(blake3::Hasher::new(), 0);
    open_segment(
        &root.segment_key(),
        &sealed,
        &SegmentExpectation {
            id: info.id,
            sealed_len: info.sealed_len,
            plain_len: info.plain_len,
            dictionary: None,
        },
        &limits,
        &mut sink,
    )
    .expect("open under the address-space limit");
    assert_eq!(sink.1, PLAIN_BYTES);
    assert_eq!(sink.0.finalize(), expected);
}

struct Noise(u64, u64);

impl Read for Noise {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.1 as usize);
        for chunk in buf[..n].chunks_mut(8) {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            chunk.copy_from_slice(&self.0.to_le_bytes()[..chunk.len()]);
        }
        self.1 -= n as u64;
        Ok(n)
    }
}

fn child_incompressible() {
    setrlimit(Resource::RLIMIT_AS, LIMIT_BYTES, LIMIT_BYTES).expect("setrlimit");
    let root = RootKey::from_bytes([9; 32]);
    let limits = Limits::new(1024 * 1024 * 1024).expect("limits");
    let result = seal_segment(
        &root.segment_key(),
        Noise(0x9e37_79b9_7f4a_7c15, 1024 * 1024 * 1024),
        &SealOptions::default(),
        &limits,
        Vec::new(),
    );
    assert_eq!(
        result,
        Err(oxabl_cache_crypto::Error::Limit("segment size"))
    );
}

fn child_control() {
    setrlimit(Resource::RLIMIT_AS, LIMIT_BYTES, LIMIT_BYTES).expect("setrlimit");
    let mut copy: Vec<u8> = Vec::new();
    assert!(
        copy.try_reserve_exact(PLAIN_BYTES as usize).is_err(),
        "the limit does not bite"
    );
}

fn run_child(mode: &str, test: &str) -> bool {
    Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", test, "--test-threads=1", "--nocapture"])
        .env(MARK, mode)
        .env("MALLOC_ARENA_MAX", "1")
        .status()
        .expect("child")
        .success()
}

#[test]
fn four_hundred_mebibytes_of_plaintext_seal_and_open_within_the_limit() {
    match std::env::var(MARK).as_deref() {
        Ok("run") => child_seal_and_open(),
        Ok(_) => {}
        Err(_) => assert!(run_child(
            "run",
            "four_hundred_mebibytes_of_plaintext_seal_and_open_within_the_limit"
        )),
    }
}

#[test]
fn the_address_space_limit_would_stop_a_full_copy_of_the_plaintext() {
    match std::env::var(MARK).as_deref() {
        Ok("control") => child_control(),
        Ok(_) => {}
        Err(_) => assert!(run_child(
            "control",
            "the_address_space_limit_would_stop_a_full_copy_of_the_plaintext"
        )),
    }
}

#[test]
fn a_gibibyte_that_cannot_compress_is_refused_without_buffering_it() {
    match std::env::var(MARK).as_deref() {
        Ok("incompressible") => child_incompressible(),
        Ok(_) => {}
        Err(_) => assert!(run_child(
            "incompressible",
            "a_gibibyte_that_cannot_compress_is_refused_without_buffering_it"
        )),
    }
}

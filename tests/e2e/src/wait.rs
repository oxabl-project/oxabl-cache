//! Readiness waits and launch retries.

use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::supervise::Supervised;

/// Polls until something listens on `port`, failing early if `process` exits.
pub fn for_port(process: &mut Supervised, port: u16, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        process.ensure_running()?;
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "port {port} not ready after {timeout:?}:\n{}",
                process.log_tail()
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Polls `check` until it returns `Ok`, failing with its last error after `timeout`.
pub fn until<T>(timeout: Duration, mut check: impl FnMut() -> Result<T>) -> Result<T> {
    let deadline = Instant::now() + timeout;
    loop {
        match check() {
            Ok(value) => return Ok(value),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Runs `launch` up to `attempts` times, so a lost race for a port is not a failure.
pub fn retry<T>(attempts: u32, mut launch: impl FnMut() -> Result<T>) -> Result<T> {
    let mut last = None;
    for _ in 0..attempts {
        match launch() {
            Ok(value) => return Ok(value),
            Err(error) => last = Some(error),
        }
    }
    Err(last.expect("at least one attempt"))
}

//! A thread that lives as long as the process, so children spawned on it outlive any one test.

use std::sync::mpsc;
use std::thread;

use anyhow::{Result, anyhow};

/// Runs `init` on a dedicated thread that then parks forever. Children spawned by `init` die
/// with that thread, which is the process, instead of with the short-lived test thread that
/// happened to ask first.
pub fn run<T: Send + 'static>(
    name: &str,
    init: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || match init() {
            Ok(value) => {
                let _ = tx.send(Ok(value));
                park_forever()
            }
            Err(error) => {
                let _ = tx.send(Err(error));
            }
        })?;
    rx.recv().map_err(|_| anyhow!("{name} thread died"))?
}

fn park_forever() -> ! {
    loop {
        thread::park();
    }
}

//! Per-test working directories under `target/e2e-tmp/`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

static SWEEP: Once = Once::new();
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A directory named `<test>-<launcher><n>-<pid>`, removed on drop unless the test panicked.
/// Directories of processes that no longer exist are removed when the first one is created.
pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub fn new(launcher: &str) -> io::Result<Self> {
        let root = root();
        fs::create_dir_all(&root)?;
        SWEEP.call_once(|| sweep_stale(&root));
        let current = thread::current();
        let test: String = current
            .name()
            .unwrap_or("unnamed")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(60)
            .collect();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!("{test}-{launcher}{n}-{}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if !thread::panicking() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/e2e-tmp")
        .components()
        .collect()
}

fn sweep_stale(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid = name
            .to_string_lossy()
            .rsplit('-')
            .next()
            .and_then(|p| p.parse::<u32>().ok());
        if pid.is_some_and(|pid| !Path::new(&format!("/proc/{pid}")).exists()) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

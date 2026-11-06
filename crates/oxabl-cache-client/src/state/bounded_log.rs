//! A private JSON-lines file that keeps only its newest entries, safe to share between
//! processes: writers take turns under a lock and replace the file atomically.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use nix::fcntl::{Flock, FlockArg, OFlag};
use serde::Serialize;
use serde::de::DeserializeOwned;

const LOCK_WAIT: Duration = Duration::from_millis(250);
const LOCK_POLL: Duration = Duration::from_millis(5);

/// How much a log keeps; older entries are dropped first.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds {
    pub entries: usize,
    pub bytes: usize,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "bounded log")
}

fn open_plain(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    Ok(file)
}

/// The entries of the log, oldest first; lines that do not parse are skipped.
pub(crate) fn read<T: DeserializeOwned>(path: &Path, bounds: Bounds) -> io::Result<Vec<T>> {
    let file = match open_plain(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut text = String::new();
    file.take(2 * bounds.bytes as u64)
        .read_to_string(&mut text)
        .map_err(|_| invalid())?;
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

fn lock(path: &Path) -> io::Result<Flock<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .open(path)?;
    let deadline = Instant::now() + LOCK_WAIT;
    let mut file = file;
    loop {
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(guard) => return Ok(guard),
            Err((back, nix::errno::Errno::EWOULDBLOCK)) if Instant::now() < deadline => {
                file = back;
                sleep(LOCK_POLL);
            }
            Err((_, errno)) => return Err(io::Error::from(errno)),
        }
    }
}

/// Lets `change` edit the entries under the lock, keeps the newest within `bounds` and replaces
/// the file atomically, so a reader sees the log before or after, and concurrent writers take
/// turns. Returns what `change` returned.
pub(crate) fn update<T, R>(
    path: &Path,
    bounds: Bounds,
    change: impl FnOnce(&mut Vec<T>) -> R,
) -> io::Result<R>
where
    T: Serialize + DeserializeOwned,
{
    let dir = path.parent().ok_or_else(invalid)?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let name = path.file_name().ok_or_else(invalid)?.to_string_lossy();
    let _turn = lock(&dir.join(format!(".{name}.lock")))?;

    let mut kept = read(path, bounds).unwrap_or_default();
    let answer = change(&mut kept);
    let mut lines: Vec<String> = kept
        .iter()
        .filter_map(|entry| serde_json::to_string(entry).ok())
        .collect();
    let mut bytes: usize = lines.iter().map(|line| line.len() + 1).sum();
    let mut drop_count = lines.len().saturating_sub(bounds.entries);
    for line in &lines[..drop_count] {
        bytes -= line.len() + 1;
    }
    while bytes > bounds.bytes && drop_count + 1 < lines.len() {
        bytes -= lines[drop_count].len() + 1;
        drop_count += 1;
    }
    lines.drain(..drop_count);

    let staging = dir.join(format!(".{name}.tmp"));
    let _ = fs::remove_file(&staging);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)?;
    let written = lines
        .iter()
        .try_for_each(|line| writeln!(file, "{line}"))
        .and_then(|()| file.flush())
        .and_then(|()| fs::rename(&staging, path));
    if written.is_err() {
        let _ = fs::remove_file(&staging);
    }
    written.map(|()| answer)
}

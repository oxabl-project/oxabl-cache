use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 2_000_000;

/// Every regular, non-empty file under `dir`. Symlinks are never followed.
pub(super) fn regular_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            seen += 1;
            if seen > MAX_ENTRIES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "too many entries under the outputs directory",
                ));
            }
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.metadata()?.len() > 0 {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// A uniform shuffle from the operating system's random generator.
pub(super) fn shuffle<T>(items: &mut [T]) -> io::Result<()> {
    for i in (1..items.len()).rev() {
        let mut word = [0u8; 8];
        getrandom::fill(&mut word).map_err(|_| io::Error::other("random generator"))?;
        let j = (u64::from_le_bytes(word) % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
    Ok(())
}

/// The first `limit` bytes of a file.
pub(super) fn head(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

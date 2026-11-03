use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use nix::fcntl::{Flock, FlockArg};

const FILE: &str = "roster-high-water";
const MAX_BYTES: u64 = 64;

/// The highest roster version this machine has verified for one cache.
pub(crate) struct HighWater {
    dir: Option<PathBuf>,
}

fn parse(text: &str) -> io::Result<u64> {
    text.trim_end_matches('\n')
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "high-water file"))
}

impl HighWater {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir }
    }

    /// The stored version; zero when there is no state yet. A file that cannot be read or
    /// parsed is an error, never a zero.
    pub fn read(&self) -> io::Result<u64> {
        let Some(dir) = &self.dir else { return Ok(0) };
        let file = match File::open(dir.join(FILE)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "high-water file",
            ));
        }
        parse(&io::read_to_string(file)?)
    }

    /// Raises the stored version to `version` if it is higher. Never lowers it.
    pub fn advance(&self, version: u64) -> io::Result<()> {
        let Some(dir) = &self.dir else { return Ok(()) };
        fs::create_dir_all(dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(dir.join("lock"))?;
        let _guard =
            Flock::lock(lock, FlockArg::LockExclusive).map_err(|(_, e)| io::Error::from(e))?;
        if self.read()? >= version {
            return Ok(());
        }
        let tmp = dir.join(format!("{FILE}.tmp"));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)?;
        writeln!(file, "{version}")?;
        file.sync_all()?;
        fs::rename(&tmp, dir.join(FILE))
    }
}

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use cap_std::ambient_authority;
use cap_std::fs::Dir;

use crate::artifact_path::ArtifactPath;

/// A stage directory, held as a handle so that nothing can be created outside it.
pub(crate) struct Stage {
    root: Dir,
}

fn not_a_directory() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotADirectory,
        "a path component is not a directory",
    )
}

impl Stage {
    pub fn open(path: &Path) -> io::Result<Self> {
        fs::create_dir_all(path)?;
        Ok(Self {
            root: Dir::open_ambient_dir(path, ambient_authority())?,
        })
    }

    /// The device the stage lives on.
    pub fn device(&self) -> io::Result<u64> {
        Ok(self.root.open(".")?.into_std().metadata()?.dev())
    }

    /// Flushes the whole file system the stage lives on.
    pub fn sync_filesystem(&self) -> io::Result<()> {
        let handle = self.root.open(".")?.into_std();
        nix::unistd::syncfs(&handle).map_err(io::Error::from)
    }

    /// The directory holding `path` and the file name inside it. Missing directories are
    /// created; every component is opened beneath the stage, and a symlink is refused.
    pub fn parent_of<'p>(&self, path: &'p ArtifactPath) -> io::Result<(Dir, &'p str)> {
        let mut parts: Vec<&str> = path.as_str().split('/').collect();
        let name = parts.pop().ok_or_else(not_a_directory)?;
        let mut dir = self.root.try_clone()?;
        for part in parts {
            match dir.symlink_metadata(part) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(not_a_directory()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => match dir.create_dir(part) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                },
                Err(e) => return Err(e),
            }
            dir = dir.open_dir(part)?;
        }
        Ok((dir, name))
    }
}

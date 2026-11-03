use std::collections::HashSet;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cap_std::fs::{Dir, File, OpenOptions};

use super::Durability;
use super::stage::Stage;
use crate::artifact_path::ArtifactPath;
use crate::fetch::InstalledFile;

struct Slot {
    path: ArtifactPath,
    dir: Dir,
    name: String,
    tmp: String,
    size: u64,
    file: Option<File>,
    renamed: bool,
}

fn temp_name(name: &str) -> io::Result<String> {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| io::Error::other("random"))?;
    let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(".{name}.oxc-{suffix}.tmp"))
}

/// Receives the concatenated files of one set and writes each to a temporary file beside its
/// final path. Dropping it, or the [`StagedSet`] it becomes, removes what was never renamed.
pub(crate) struct SetWriter {
    slots: Vec<Slot>,
    current: usize,
    written: u64,
    cancel: Arc<AtomicBool>,
}

/// Every file of a set, written completely and checked, not yet at its final path.
pub(crate) struct StagedSet {
    slots: Vec<Slot>,
}

impl SetWriter {
    /// Creates the temporary files for `files`, in order.
    pub fn create(
        stage: &Stage,
        files: &[(ArtifactPath, u64)],
        cancel: Arc<AtomicBool>,
    ) -> io::Result<Self> {
        let mut writer = Self {
            slots: Vec::with_capacity(files.len()),
            current: 0,
            written: 0,
            cancel,
        };
        for (path, size) in files {
            let (dir, name) = stage.parent_of(path)?;
            let tmp = temp_name(name)?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let file = dir.open_with(&tmp, &options)?;
            writer.slots.push(Slot {
                path: path.clone(),
                name: name.to_owned(),
                dir,
                tmp,
                size: *size,
                file: Some(file),
                renamed: false,
            });
        }
        Ok(writer)
    }

    /// Ends the stream; every file must have received exactly its size.
    pub fn finish(mut self) -> io::Result<StagedSet> {
        self.skip_full();
        if self.current < self.slots.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "set is short"));
        }
        Ok(StagedSet {
            slots: std::mem::take(&mut self.slots),
        })
    }

    fn skip_full(&mut self) {
        while self.current < self.slots.len() && self.written == self.slots[self.current].size {
            self.current += 1;
            self.written = 0;
        }
    }
}

impl Write for SetWriter {
    fn write(&mut self, mut buf: &[u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        let total = buf.len();
        while !buf.is_empty() {
            self.skip_full();
            let Some(slot) = self.slots.get_mut(self.current) else {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "set is long"));
            };
            let room = (slot.size - self.written).min(buf.len() as u64) as usize;
            let file = slot.file.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
            file.write_all(&buf[..room])?;
            self.written += room as u64;
            buf = &buf[room..];
        }
        Ok(total)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for SetWriter {
    fn drop(&mut self) {
        remove_unrenamed(&mut self.slots);
    }
}

fn remove_unrenamed(slots: &mut [Slot]) {
    for slot in slots.iter_mut().filter(|s| !s.renamed) {
        slot.file = None;
        let _ = slot.dir.remove_file(&slot.tmp);
    }
}

impl StagedSet {
    /// Total bytes of the set.
    pub fn bytes(&self) -> u64 {
        self.slots.iter().map(|s| s.size).sum()
    }

    /// Flushes every temporary file to stable storage individually.
    pub fn sync_files(&self) -> io::Result<()> {
        for file in self.slots.iter().filter_map(|s| s.file.as_ref()) {
            file.sync_all()?;
        }
        Ok(())
    }

    /// Flushes the temporary files that live on another device than `device`, which a
    /// whole-device flush of `device` does not cover.
    pub fn sync_files_off_device(&self, device: u64) -> io::Result<()> {
        for file in self.slots.iter().filter_map(|s| s.file.as_ref()) {
            if file.try_clone()?.into_std().metadata()?.dev() != device {
                file.sync_all()?;
            }
        }
        Ok(())
    }

    /// Renames every temporary file over its final path.
    pub fn rename(&mut self) -> io::Result<Vec<InstalledFile>> {
        let mut installed = Vec::with_capacity(self.slots.len());
        for slot in &mut self.slots {
            slot.file = None;
            slot.dir.rename(&slot.tmp, &slot.dir, &slot.name)?;
            slot.renamed = true;
            installed.push(InstalledFile {
                path: slot.path.clone(),
                size: slot.size,
            });
        }
        Ok(installed)
    }

    /// Flushes each directory holding a file of the set, once per `seen` directory path.
    pub fn sync_dirs(&self, seen: &mut HashSet<String>) -> io::Result<()> {
        for slot in &self.slots {
            let parent = slot
                .path
                .as_str()
                .rsplit_once('/')
                .map_or("", |(dir, _)| dir);
            if !seen.contains(parent) {
                slot.dir.open(".")?.sync_all()?;
                seen.insert(parent.to_owned());
            }
        }
        Ok(())
    }

    /// Renames the files into place, fsyncing first and after when `durability` asks for it.
    pub fn commit(mut self, durability: Durability) -> io::Result<Vec<InstalledFile>> {
        if durability == Durability::PerFile {
            self.sync_files()?;
        }
        let installed = self.rename()?;
        if durability == Durability::PerFile {
            self.sync_dirs(&mut HashSet::new())?;
        }
        Ok(installed)
    }
}

impl Drop for StagedSet {
    fn drop(&mut self) {
        remove_unrenamed(&mut self.slots);
    }
}

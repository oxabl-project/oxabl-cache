//! Durable object writes: temporary file, fsync, rename, directory fsync.

use crate::error::io_error;
use crate::head::head_of;
use crate::layout::{object_path, tmp_dir};
use oxabl_cache_core::ports::{ObjectHead, ObjectKey};
use oxabl_cache_core::{Error, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A blocking writer for one object. Nothing is visible under the object's name
/// until [`ObjectWriter::finish`] has synced the data and the directory; a writer
/// dropped earlier leaves nothing behind.
#[derive(Debug)]
pub struct ObjectWriter {
    file: Option<File>,
    tmp: PathBuf,
    target: PathBuf,
    written: u64,
}

impl ObjectWriter {
    pub(crate) fn create(root: &Path, key: &ObjectKey) -> Result<Self> {
        let mut name = [0u8; 16];
        getrandom::fill(&mut name).map_err(|e| Error::internal(format!("randomness: {e}")))?;
        let tmp = tmp_dir(root).join(oxabl_cache_core::encoding::hex_encode(&name));
        let file = File::create_new(&tmp).map_err(|e| io_error(&e))?;
        Ok(Self {
            file: Some(file),
            tmp,
            target: object_path(root, key),
            written: 0,
        })
    }

    /// Appends bytes.
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        let file = self.file.as_mut().expect("the file is open until finish");
        file.write_all(bytes).map_err(|e| io_error(&e))?;
        self.written += u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        Ok(())
    }

    /// Bytes written so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// Syncs, renames into place and syncs the directory. With `expected_size`,
    /// a different byte count is `BadRequest` and nothing becomes visible.
    pub fn finish(mut self, expected_size: Option<u64>) -> Result<ObjectHead> {
        if expected_size.is_some_and(|size| size != self.written) {
            return Err(Error::bad_request(
                "object size differs from the declared size",
            ));
        }
        let file = self.file.take().expect("the file is open until finish");
        self.publish(&file).map_err(|e| io_error(&e))
    }

    fn publish(&self, file: &File) -> std::io::Result<ObjectHead> {
        file.sync_all()?;
        let directory = self.target.parent().expect("object paths have a parent");
        fs::create_dir_all(directory)?;
        fs::rename(&self.tmp, &self.target)?;
        File::open(directory)?.sync_all()?;
        head_of(&self.target)
    }
}

impl Drop for ObjectWriter {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.tmp);
    }
}

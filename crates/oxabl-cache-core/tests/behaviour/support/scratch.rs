//! A scratch directory under the target directory, removed on drop.

use oxabl_cache_core::ids::NodeId;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(NodeId::random().to_string());
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

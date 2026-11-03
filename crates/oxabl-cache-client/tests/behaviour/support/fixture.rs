//! Synthetic artifact sets that look like r-code: a header, a string table and a compressible
//! body, each carrying a canary unique to its key.

use std::fs;
use std::path::{Path, PathBuf};

use oxabl_cache_client::{ArtifactFileRef, ArtifactPath, CompileKey};

/// One file of a set: its artifact path, where it lives and what it holds.
#[derive(Clone)]
pub struct Made {
    pub path: String,
    pub disk: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct Set {
    pub key: CompileKey,
    pub files: Vec<Made>,
}

impl Set {
    pub fn refs(&self) -> Vec<ArtifactFileRef> {
        self.files
            .iter()
            .map(|f| ArtifactFileRef {
                path: ArtifactPath::new(&f.path).expect("path"),
                disk_path: f.disk.clone(),
                size: f.bytes.len() as u64,
                blake3: *blake3::hash(&f.bytes).as_bytes(),
            })
            .collect()
    }

    pub fn paths(&self) -> Vec<ArtifactPath> {
        self.files
            .iter()
            .map(|f| ArtifactPath::new(&f.path).expect("path"))
            .collect()
    }
}

fn noise(seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    blake3::Hasher::new()
        .update(seed)
        .finalize_xof()
        .fill(&mut out);
    out
}

/// Bytes that compress about 3x, unique to `seed`.
pub fn rcode_like(seed: &str, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(b"\x1d\x04rcode\x00");
    out.extend_from_slice(format!("canary:{seed}\0").as_bytes());
    let table = format!("ar/customer.p\0ar/order.i\0{seed}\0");
    while out.len() < len * 2 / 3 {
        out.extend_from_slice(table.as_bytes());
    }
    out.truncate(len * 2 / 3);
    out.extend_from_slice(&noise(seed.as_bytes(), len - out.len()));
    out
}

/// The compile key of the `n`th set.
pub fn key(n: usize) -> CompileKey {
    CompileKey(*blake3::hash(format!("compile key {n}").as_bytes()).as_bytes())
}

/// Writes `count` sets under `root`, in the default layout: `r/…`, `xref/…`, `dbg/…`.
pub fn sets(root: &Path, count: usize, seed: &str) -> Vec<Set> {
    layout(root, count, seed, ("r", "xref", "dbg"))
}

/// Writes sets whose files live under `dirs` (r-code, XREF, debug list) on disk while their
/// artifact paths stay `r/…`, `xref/…` and `dbg/…`.
pub fn layout(root: &Path, count: usize, seed: &str, dirs: (&str, &str, &str)) -> Vec<Set> {
    (0..count)
        .map(|n| {
            let size = 2_000 + (n * 7_919) % 90_000;
            let parts = [
                ("r", dirs.0, "r", size),
                ("xref", dirs.1, "xml", size / 4 + 100),
                ("dbg", dirs.2, "dbg", size / 8 + 50),
            ];
            let files = parts
                .iter()
                .take(if n % 3 == 0 { 3 } else { 2 })
                .map(|(logical, disk, ext, len)| {
                    let name = format!("ar/unit{n}.{ext}");
                    let bytes = rcode_like(&format!("{seed}-{n}-{logical}"), *len);
                    let path = root.join(disk).join(&name);
                    fs::create_dir_all(path.parent().expect("parent")).expect("directory");
                    fs::write(&path, &bytes).expect("write");
                    Made {
                        path: format!("{logical}/{name}"),
                        disk: path,
                        bytes,
                    }
                })
                .collect();
            Set { key: key(n), files }
        })
        .collect()
}

/// Text drawn from a shared vocabulary, so that a trained dictionary pays off.
pub fn vocabulary_text(seed: &str, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 16);
    let mut counter = 0u64;
    while out.len() < len {
        let pick = blake3::hash(format!("{seed}/{counter}").as_bytes());
        let word = u16::from_le_bytes([pick.as_bytes()[0], pick.as_bytes()[1]]) % 400;
        out.extend_from_slice(
            format!("proc_{word:03}_{}(x);\n", word.wrapping_mul(2_654) % 997).as_bytes(),
        );
        counter += 1;
    }
    out.truncate(len);
    out
}

/// Writes `count` single-file sets of vocabulary text of about `size` bytes.
pub fn text_sets(root: &Path, count: usize, seed: &str, size: usize) -> Vec<Set> {
    (0..count)
        .map(|n| {
            let bytes = vocabulary_text(&format!("{seed}-{n}"), size + (n * 37) % 512);
            let path = root.join("r").join(format!("ar/{seed}{n}.r"));
            fs::create_dir_all(path.parent().expect("parent")).expect("directory");
            fs::write(&path, &bytes).expect("write");
            Set {
                key: CompileKey(*blake3::hash(format!("{seed} key {n}").as_bytes()).as_bytes()),
                files: vec![Made {
                    path: format!("r/ar/{seed}{n}.r"),
                    disk: path,
                    bytes,
                }],
            }
        })
        .collect()
}

/// The same sets under other compile keys: key `n` becomes key `n + offset`.
pub fn rekey(sets: Vec<Set>, offset: usize) -> Vec<Set> {
    sets.into_iter()
        .enumerate()
        .map(|(n, set)| Set {
            key: key(n + offset),
            ..set
        })
        .collect()
}

/// One set with one r-code file of `len` bytes.
pub fn single(root: &Path, seed: &str, len: usize) -> Set {
    let bytes = rcode_like(seed, len);
    let path = root.join("r").join(format!("ar/{seed}.r"));
    fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    fs::write(&path, &bytes).expect("write");
    Set {
        key: CompileKey(*blake3::hash(seed.as_bytes()).as_bytes()),
        files: vec![Made {
            path: format!("r/ar/{seed}.r"),
            disk: path,
            bytes,
        }],
    }
}

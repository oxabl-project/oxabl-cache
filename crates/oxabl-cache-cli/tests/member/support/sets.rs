//! Artifact sets on disk, as oxabl would have after a compile.

use std::fs;
use std::path::Path;

use oxabl_cache_client::{ArtifactFileRef, ArtifactPath, CompileKey};

pub struct Set {
    pub key: CompileKey,
    pub files: Vec<(String, Vec<u8>)>,
    pub refs: Vec<ArtifactFileRef>,
}

fn bytes(label: &str, len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    blake3::Hasher::new()
        .update(label.as_bytes())
        .finalize_xof()
        .fill(&mut out);
    out
}

/// `count` sets whose two files each are written under `dir`; `label` makes them distinct.
pub fn make(dir: &Path, label: &str, count: usize) -> Vec<Set> {
    (0..count)
        .map(|n| {
            let key = CompileKey(*blake3::hash(format!("{label} key {n}").as_bytes()).as_bytes());
            let files: Vec<(String, Vec<u8>)> = ["r", "xref"]
                .iter()
                .map(|kind| {
                    (
                        format!("{kind}/{label}/{n}.{kind}"),
                        bytes(&format!("{label} {kind} {n}"), 6_000 + n * 17),
                    )
                })
                .collect();
            let refs = files
                .iter()
                .map(|(path, content)| {
                    let disk = dir.join(path);
                    fs::create_dir_all(disk.parent().expect("parent")).expect("directory");
                    fs::write(&disk, content).expect("file");
                    ArtifactFileRef::from_disk(ArtifactPath::new(path).expect("path"), disk)
                        .expect("measured")
                })
                .collect();
            Set { key, files, refs }
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

/// `count` files under `dir`, each of `size` bytes: vocabulary text, or noise.
pub fn write_outputs(dir: &Path, seed: &str, count: usize, size: usize, vocabulary: bool) {
    for n in 0..count {
        let content = if vocabulary {
            vocabulary_text(&format!("{seed}-{n}"), size)
        } else {
            bytes(&format!("{seed}-{n}"), size)
        };
        let path = dir.join(format!("ar/{n}.r"));
        fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        fs::write(path, content).expect("output");
    }
}

/// `count` single-file sets of vocabulary text of about `size` bytes.
pub fn make_text(dir: &Path, label: &str, count: usize, size: usize) -> Vec<Set> {
    (0..count)
        .map(|n| {
            let key = CompileKey(*blake3::hash(format!("{label} key {n}").as_bytes()).as_bytes());
            let path = format!("r/{label}/{n}.r");
            let content = vocabulary_text(&format!("{label}-{n}"), size + (n * 37) % 512);
            let disk = dir.join(&path);
            fs::create_dir_all(disk.parent().expect("parent")).expect("directory");
            fs::write(&disk, &content).expect("file");
            let refs = vec![
                ArtifactFileRef::from_disk(ArtifactPath::new(&path).expect("path"), disk)
                    .expect("measured"),
            ];
            Set {
                key,
                files: vec![(path, content)],
                refs,
            }
        })
        .collect()
}

/// The compile key as the command line spells it.
pub fn key_hex(key: &CompileKey) -> String {
    key.0.iter().map(|b| format!("{b:02x}")).collect()
}

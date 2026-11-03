//! The client's path from a compile key to verified bytes, through the public API only.

use std::io::Read;

use oxabl_cache_crypto::{
    EntryFields, Error, FileEntry, Identity, SealOptions, open_record, open_segment, seal_record,
    seal_segment,
};
use oxabl_cache_format::{Limits, Namespace};

use crate::support::data::{Rng, rcode_like};
use crate::support::world::World;

struct Stored {
    record: Vec<u8>,
    segment: Vec<u8>,
}

fn files() -> Vec<(String, Vec<u8>)> {
    let mut rng = Rng::new(90);
    vec![
        ("r/ar/customer.r".into(), rcode_like(&mut rng, 120_000)),
        ("xref/ar/customer.xml".into(), rcode_like(&mut rng, 30_000)),
        ("dbg/ar/customer.dbg".into(), rcode_like(&mut rng, 5_000)),
    ]
}

fn store(w: &World, namespace: Namespace, signer: &Identity, compile_key: &[u8; 32]) -> Stored {
    let files = files();
    let stream = files
        .iter()
        .map(|(_, bytes)| std::io::Cursor::new(bytes.clone()))
        .fold(Box::new(std::io::empty()) as Box<dyn Read>, |acc, next| {
            Box::new(acc.chain(next))
        });
    let mut segment = Vec::new();
    let info = seal_segment(
        &w.root.segment_key(),
        stream,
        &SealOptions::default(),
        &Limits::default(),
        &mut segment,
    )
    .expect("seal");
    let lookup_id = w.root.lookup_key().lookup_id(compile_key);
    let fields = EntryFields {
        segment_id: info.id,
        segment_len: info.sealed_len as u32,
        dict_id: None,
        plain_len: info.plain_len,
        created_at: 1_793_000_000,
        roster_version: w.roster.version(),
        files: files
            .iter()
            .map(|(p, b)| FileEntry {
                path: p.clone(),
                size: b.len() as u64,
            })
            .collect(),
    };
    let record = seal_record(
        &w.root.record_key(),
        &w.context(namespace, lookup_id),
        &fields,
        signer,
        &Limits::default(),
    )
    .expect("record");
    Stored { record, segment }
}

fn fetch(
    w: &World,
    reader: &Identity,
    namespace: Namespace,
    compile_key: &[u8; 32],
    stored: &Stored,
) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let root = w.roster.unwrap_root_key(reader)?;
    let lookup_id = root.lookup_key().lookup_id(compile_key);
    let accepted = open_record(
        &root.record_key(),
        &w.context(namespace, lookup_id),
        &stored.record,
        &Limits::default(),
    )?
    .accept(&w.roster, &reader.signing_public())?;
    let mut plain = Vec::new();
    open_segment(
        &root.segment_key(),
        &stored.segment,
        &accepted.segment_expectation(None)?,
        &Limits::default(),
        &mut plain,
    )?;
    let mut rest = &plain[..];
    let mut out = Vec::new();
    for f in &accepted.entry().fields.files {
        let (head, tail) = rest.split_at(f.size as usize);
        out.push((f.path.clone(), head.to_vec()));
        rest = tail;
    }
    assert!(rest.is_empty());
    Ok(out)
}

#[test]
fn ci_stores_and_a_developer_fetches_verified_files() {
    let w = World::new();
    let key = [0x42; 32];
    let stored = store(&w, Namespace::Trusted, &w.ci, &key);
    assert_eq!(
        fetch(&w, &w.dev, Namespace::Trusted, &key, &stored).expect("fetch"),
        files()
    );
    assert_eq!(
        fetch(&w, &w.reader, Namespace::Trusted, &key, &stored).expect("fetch"),
        files()
    );
}

#[test]
fn a_developer_write_to_the_trusted_namespace_is_never_installed() {
    let w = World::new();
    let key = [0x43; 32];
    let stored = store(&w, Namespace::Trusted, &w.dev2, &key);
    assert_eq!(
        fetch(&w, &w.dev, Namespace::Trusted, &key, &stored),
        Err(Error::SignerNotAccepted)
    );
}

#[test]
fn personal_entries_are_readable_by_their_author_alone() {
    let w = World::new();
    let key = [0x44; 32];
    let stored = store(&w, Namespace::Personal, &w.dev, &key);
    assert_eq!(
        fetch(&w, &w.dev, Namespace::Personal, &key, &stored).expect("fetch"),
        files()
    );
    assert_eq!(
        fetch(&w, &w.dev2, Namespace::Personal, &key, &stored),
        Err(Error::SignerNotAccepted)
    );
}

#[test]
fn a_record_swapped_onto_another_key_misses() {
    let w = World::new();
    let (a, b) = ([0x45; 32], [0x46; 32]);
    let stored = store(&w, Namespace::Trusted, &w.ci, &a);
    assert_eq!(
        fetch(&w, &w.dev, Namespace::Trusted, &b, &stored),
        Err(Error::Authentication)
    );
}

#[test]
fn a_segment_swapped_between_two_entries_is_caught_by_the_signed_id() {
    let w = World::new();
    let (a, b) = ([0x47; 32], [0x48; 32]);
    let first = store(&w, Namespace::Trusted, &w.ci, &a);
    let second = store(&w, Namespace::Trusted, &w.ci, &b);
    let swapped = Stored {
        record: first.record.clone(),
        segment: second.segment.clone(),
    };
    assert_eq!(
        fetch(&w, &w.dev, Namespace::Trusted, &a, &swapped),
        Err(Error::SegmentId)
    );
}

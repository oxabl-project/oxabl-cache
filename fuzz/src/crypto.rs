//! Bodies of the crypto fuzz targets: every open and verify path on attacker-controlled bytes.

use std::io::Write;

use libfuzzer_sys::arbitrary::Unstructured;
use oxabl_cache_crypto::identity_file::unprotect_with_max_work_factor;
use oxabl_cache_crypto::identity_string::parse_descriptor;
use oxabl_cache_crypto::{
    Dictionary, EntryFields, ExposeSecret, FileEntry, Identity, SecretString, SegmentExpectation, VerifiedRoster,
    open_record, open_segment, seal_compressed, seal_record, sign_pointer, verify_pointer,
};
use oxabl_cache_format::{Codec, Limits, SegmentId};

use crate::crypto_fixture::fixture;

struct Count(u64);

impl Write for Count {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn id_of(bytes: &[u8]) -> SegmentId {
    SegmentId(*blake3::hash(bytes).as_bytes())
}

fn open_counted(sealed: &[u8], plain_len: u64, dictionary: Option<&Dictionary>) {
    let key = fixture().root.segment_key();
    let mut sink = Count(0);
    let _ = open_segment(
        &key,
        sealed,
        &SegmentExpectation {
            id: id_of(sealed),
            sealed_len: sealed.len() as u64,
            plain_len,
            dictionary,
        },
        &Limits::default(),
        &mut sink,
    );
    assert!(sink.0 <= plain_len, "wrote {} bytes for a signed length of {plain_len}", sink.0);
}

/// Segments: raw bytes, and attacker-chosen compressed streams sealed under the real key.
pub fn segment(data: &[u8]) {
    let [mode, a, b, c, d, rest @ ..] = data else {
        return;
    };
    let plain_len = u64::from(u32::from_be_bytes([a & 0x03, *b, *c, *d]));
    match mode % 3 {
        0 => open_counted(rest, plain_len, None),
        1 => {
            let mut sealed = Vec::new();
            if seal_compressed(&fixture().root.segment_key(), Codec::Zstd, rest, &mut sealed).is_ok() {
                open_counted(&sealed, plain_len, None);
            }
        }
        _ => {
            let split = usize::from(*a) % (rest.len() + 1);
            let (dict_bytes, frame) = rest.split_at(split);
            let Ok(dictionary) = Dictionary::new(SegmentId([1; 32]), dict_bytes.to_vec(), 3) else {
                return;
            };
            let mut sealed = Vec::new();
            if seal_compressed(&fixture().root.segment_key(), Codec::ZstdDictionary, frame, &mut sealed).is_ok() {
                open_counted(&sealed, plain_len, Some(&dictionary));
            }
        }
    }
}

fn fields(u: &mut Unstructured<'_>) -> Option<EntryFields> {
    let count = u.int_in_range(0..=17).ok()?;
    let files = (0..count)
        .map(|_| {
            Some(FileEntry {
                path: u.arbitrary::<String>().ok()?,
                size: u.int_in_range(0..=1 << 20).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(EntryFields {
        segment_id: SegmentId(u.arbitrary().ok()?),
        segment_len: u.arbitrary().ok()?,
        dict_id: u.arbitrary::<bool>().ok()?.then_some(SegmentId(u.arbitrary().ok()?)),
        plain_len: files.iter().map(|f| f.size).sum(),
        created_at: u.arbitrary().ok()?,
        roster_version: u.arbitrary().ok()?,
        files,
    })
}

/// Records: raw bytes, and a seal-then-open round trip of arbitrary fields.
pub fn record(data: &[u8]) {
    let f = fixture();
    let key = f.root.record_key();
    let ctx = f.context();
    let limits = Limits::default();
    let _ = open_record(&key, &ctx, data, &limits);

    if let Some(fields) = fields(&mut Unstructured::new(data)) {
        if let Ok(sealed) = seal_record(&key, &ctx, &fields, &f.member, &limits) {
            assert_eq!(sealed.len() % 256, 0);
            let opened = open_record(&key, &ctx, &sealed, &limits).expect("a sealed record opens");
            assert_eq!(opened.signer(), f.member.signing_public());
            let mut other = ctx;
            other.epoch += 1;
            assert!(open_record(&key, &other, &sealed, &limits).is_err());
        }
    }
}

/// Rosters: raw bytes, and edits of a valid roster, which verify only if they change nothing.
pub fn roster(data: &[u8]) {
    let f = fixture();
    if let Ok(verified) = VerifiedRoster::verify(data, &f.pins, 0) {
        let _ = verified.unwrap_root_key(&f.member);
    }
    let mut edited = f.roster_bytes.clone();
    for edit in data.chunks_exact(3) {
        let at = usize::from(u16::from_be_bytes([edit[0], edit[1]])) % edited.len();
        edited[at] ^= edit[2];
    }
    if let Ok(verified) = VerifiedRoster::verify(&edited, &f.pins, 0) {
        assert_eq!(verified.as_bytes(), &f.roster_bytes[..], "an altered roster verified");
    }
}

/// Identity and descriptor strings.
pub fn identity(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    if let Ok(identity) = Identity::parse(&text) {
        let secret = identity.to_secret_string().expect("an identity encodes");
        let again = Identity::parse(secret.expose_secret()).expect("an identity string re-parses");
        assert_eq!(again.signing_public(), identity.signing_public());
    }
    let _ = parse_descriptor(&text);
}

/// Passphrase-protected identity files, with the scrypt cost capped so the fuzzer stays fast.
pub fn identity_file(data: &[u8]) {
    let _ = unprotect_with_max_work_factor(data, &SecretString::from("x".to_owned()), 4);
}

/// Dictionary pointers: only the exact signed bytes verify.
pub fn pointer(data: &[u8]) {
    let f = fixture();
    let valid = sign_pointer(&f.member, f.cache_id, f.roster.epoch(), 1, SegmentId([9; 32])).encode();
    if verify_pointer(data, &f.roster).is_ok() {
        assert_eq!(data, &valid[..]);
    }
    let mut edited = valid.clone();
    for edit in data.chunks_exact(2) {
        let at = usize::from(edit[0]) % edited.len();
        edited[at] ^= edit[1];
    }
    if verify_pointer(&edited, &f.roster).is_ok() {
        assert_eq!(edited, valid);
    }
}

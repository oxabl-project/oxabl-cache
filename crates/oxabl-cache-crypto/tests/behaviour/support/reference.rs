//! An independent implementation of the byte formats in protocol.md and security.md, used to
//! check the product against the specification and to craft hostile but well-formed inputs.

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};

pub const CHUNK: usize = 1 << 16;
pub const TAG: usize = 16;
pub const SEGMENT_HEADER: usize = 72;

pub fn padme(len: usize) -> usize {
    if len < 2 {
        return len;
    }
    let e = len.ilog2();
    let s = e.ilog2() + 1;
    let mask = (1usize << (e - s)) - 1;
    (len + mask) & !mask
}

pub fn sub_keys(root: &[u8; 32]) -> ([u8; 32], [u8; 32], [u8; 32]) {
    (
        blake3::derive_key("oxabl-cache v1 lookup", root),
        blake3::derive_key("oxabl-cache v1 segment", root),
        blake3::derive_key("oxabl-cache v1 record", root),
    )
}

pub fn object_keys(sub_key: &[u8; 32], salt: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let mut material = [0u8; 64];
    material[..32].copy_from_slice(sub_key);
    material[32..].copy_from_slice(salt);
    (
        blake3::derive_key("oxabl-cache v1 enc", &material),
        blake3::derive_key("oxabl-cache v1 commit", &material),
    )
}

pub fn segment_header(salt: &[u8; 32], commit: &[u8; 32]) -> [u8; SEGMENT_HEADER] {
    let mut h = [0u8; SEGMENT_HEADER];
    h[..4].copy_from_slice(b"OXSG");
    h[4] = 1;
    h[5] = 1;
    h[6] = 16;
    h[8..40].copy_from_slice(salt);
    h[40..72].copy_from_slice(commit);
    h
}

pub fn nonce(index: u64, last: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[3..11].copy_from_slice(&index.to_be_bytes());
    n[11] = u8::from(last);
    n
}

fn gcm(key: &[u8; 32]) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("key"))
}

/// `OXSP` plaintext: header, compressed bytes and zero padding to Padmé of their length.
pub fn oxsp(codec: u8, compressed: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"OXSP");
    out.push(1);
    out.push(codec);
    out.extend_from_slice(&(compressed.len() as u64).to_be_bytes());
    out.extend_from_slice(compressed);
    out.resize(padme(out.len()), 0);
    out
}

/// Seals a plaintext as `OXSG` with explicit keys and salt.
pub fn seal_stream(
    enc: &[u8; 32],
    commit: &[u8; 32],
    salt: &[u8; 32],
    plaintext: &[u8],
) -> Vec<u8> {
    let header = segment_header(salt, commit);
    let aad = blake3::hash(&header);
    let key = gcm(enc);
    let mut out = header.to_vec();
    let chunks: Vec<&[u8]> = plaintext.chunks(CHUNK).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let mut buf = chunk.to_vec();
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce(i as u64, i + 1 == chunks.len())),
            Aad::from(aad.as_bytes()),
            &mut buf,
        )
        .expect("seal");
        out.extend_from_slice(&buf);
    }
    out
}

/// Seals a plaintext under the segment key of `root` and a chosen salt.
pub fn seal_segment_plaintext(root: &[u8; 32], salt: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    let (_, segment, _) = sub_keys(root);
    let (enc, commit) = object_keys(&segment, salt);
    seal_stream(&enc, &commit, salt, plaintext)
}

/// Opens an `OXSG` under the segment key of `root`; `None` on any failure.
pub fn open_segment_plaintext(root: &[u8; 32], sealed: &[u8]) -> Option<Vec<u8>> {
    let (_, segment, _) = sub_keys(root);
    let salt: [u8; 32] = sealed.get(8..40)?.try_into().ok()?;
    let (enc, commit) = object_keys(&segment, &salt);
    if sealed.get(40..72)? != commit {
        return None;
    }
    let aad = blake3::hash(&sealed[..SEGMENT_HEADER]);
    let key = gcm(&enc);
    let body = &sealed[SEGMENT_HEADER..];
    let chunks: Vec<&[u8]> = body.chunks(CHUNK + TAG).collect();
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let mut buf = chunk.to_vec();
        let plain = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce(i as u64, i + 1 == chunks.len())),
                Aad::from(aad.as_bytes()),
                &mut buf,
            )
            .ok()?;
        out.extend_from_slice(plain);
    }
    Some(out)
}

/// The associated data of a record.
pub fn record_ad(cache_id: &[u8; 16], epoch: u32, ns: u8, lookup_id: &[u8; 32]) -> Vec<u8> {
    let mut ad = b"oxabl-cache v1 record".to_vec();
    ad.extend_from_slice(cache_id);
    ad.extend_from_slice(&epoch.to_be_bytes());
    ad.push(ns);
    ad.extend_from_slice(lookup_id);
    ad
}

/// The transcript an entry signature covers, up to the payload bytes.
pub fn entry_transcript_prefix(
    cache_id: &[u8; 16],
    epoch: u32,
    ns: u8,
    lookup_id: &[u8; 32],
) -> Vec<u8> {
    let mut t = b"oxabl-cache v1 entry".to_vec();
    t.extend_from_slice(cache_id);
    t.extend_from_slice(&epoch.to_be_bytes());
    t.push(ns);
    t.extend_from_slice(lookup_id);
    t
}

/// Seals a record plaintext as `OXER` with explicit keys and salt.
pub fn seal_record_raw(
    enc: &[u8; 32],
    commit: &[u8; 32],
    salt: &[u8; 32],
    ad: &[u8],
    plaintext: &[u8],
) -> Vec<u8> {
    let mut buf = plaintext.to_vec();
    gcm(enc)
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key([0; 12]),
            Aad::from(ad),
            &mut buf,
        )
        .expect("seal");
    let mut out = Vec::new();
    out.extend_from_slice(b"OXER");
    out.extend_from_slice(&[1, 1, 0, 0]);
    out.extend_from_slice(salt);
    out.extend_from_slice(commit);
    out.extend_from_slice(&buf);
    out
}

/// Opens an `OXER` under the record key of `root`.
pub fn open_record_raw(root: &[u8; 32], ad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    let (_, _, record) = sub_keys(root);
    let salt: [u8; 32] = sealed.get(8..40)?.try_into().ok()?;
    let (enc, commit) = object_keys(&record, &salt);
    if sealed.get(40..72)? != commit {
        return None;
    }
    let mut buf = sealed[72..].to_vec();
    gcm(&enc)
        .open_in_place(
            Nonce::assume_unique_for_key([0; 12]),
            Aad::from(ad),
            &mut buf,
        )
        .ok()
        .map(|p| p.to_vec())
}

/// Sub-keys of the fixed test root used by the record tests.
pub fn object_keys_for_record() -> ([u8; 32], [u8; 32], [u8; 32]) {
    sub_keys(&[9; 32])
}

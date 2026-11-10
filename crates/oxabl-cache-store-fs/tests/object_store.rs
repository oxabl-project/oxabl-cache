//! The filesystem store through its public API: durable writes, ranges, URLs.

use oxabl_cache_core::ids::{CacheId, NodeId, PackId};
use oxabl_cache_core::ports::{ObjectHead, ObjectKey, ObjectStore};
use oxabl_cache_core::{Conflict, Error};
use oxabl_cache_store_fs::{FsStore, UrlRejection, UrlRequest};
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(NodeId::random().to_string());
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const KEY: [u8; 32] = [3; 32];

fn open(dir: &Scratch) -> FsStore {
    FsStore::open(dir.0.join("objects"), "http://cache.test/", vec![KEY]).expect("open")
}

fn key(cache: CacheId) -> ObjectKey {
    ObjectKey {
        cache,
        pack: PackId::random(),
    }
}

#[tokio::test]
async fn an_object_is_invisible_until_its_writer_finishes() {
    let dir = Scratch::new();
    let store = open(&dir);
    let key = key(CacheId::random());
    let mut writer = store.writer(&key).expect("writer");
    writer.write_all(b"hello ").expect("write");
    writer.write_all(b"world").expect("write");
    assert_eq!(writer.written(), 11);
    assert!(
        store.head(&key).await.expect("head").is_none(),
        "nothing is visible before finish"
    );
    let head = writer.finish(Some(11)).expect("finish");
    assert_eq!(head.size, 11);
    assert_eq!(store.head(&key).await.expect("head"), Some(head));
    assert_eq!(
        store.read_range(&key, 6, 5, None).await.expect("range"),
        b"world"
    );
}

#[tokio::test]
async fn a_dropped_or_mis_sized_writer_leaves_nothing_behind() {
    let dir = Scratch::new();
    let store = open(&dir);
    let cache = CacheId::random();
    let dropped = key(cache);
    let mut writer = store.writer(&dropped).expect("writer");
    writer.write_all(b"partial").expect("write");
    drop(writer);
    assert!(store.head(&dropped).await.expect("head").is_none());

    let mis_sized = key(cache);
    let mut writer = store.writer(&mis_sized).expect("writer");
    writer.write_all(b"12345").expect("write");
    assert!(matches!(writer.finish(Some(6)), Err(Error::BadRequest(_))));
    assert!(store.head(&mis_sized).await.expect("head").is_none());
    assert!(store.list(&cache).await.expect("list").is_empty());
    let leftovers = std::fs::read_dir(dir.0.join("objects/tmp"))
        .expect("tmp")
        .count();
    assert_eq!(leftovers, 0, "temporary files are removed");
}

#[tokio::test]
async fn opening_discards_temporary_files_a_crash_left_behind() {
    let dir = Scratch::new();
    let store = open(&dir);
    drop(store);
    std::fs::write(dir.0.join("objects/tmp/crashed"), b"half a pack").expect("leftover");
    let store = open(&dir);
    assert_eq!(
        std::fs::read_dir(dir.0.join("objects/tmp"))
            .expect("tmp")
            .count(),
        0
    );
    assert!(
        store
            .list(&CacheId::random())
            .await
            .expect("list")
            .is_empty()
    );
}

#[tokio::test]
async fn ranges_check_bounds_and_the_etag_and_a_replaced_object_changes_its_etag() {
    let dir = Scratch::new();
    let store = open(&dir);
    let key = key(CacheId::random());
    let first = store.put(&key, vec![1; 100]).await.expect("put");
    assert_eq!(
        store
            .read_range(&key, 90, 10, Some(&first.etag))
            .await
            .expect("range"),
        vec![1; 10]
    );
    assert!(
        matches!(
            store.read_range(&key, 95, 10, None).await,
            Err(Error::BadRequest(_))
        ),
        "past the end"
    );
    assert!(
        matches!(
            store.read_range(&key, u64::MAX, 2, None).await,
            Err(Error::BadRequest(_))
        ),
        "overflow"
    );

    let second = store.put(&key, vec![2; 100]).await.expect("replace");
    assert_ne!(first.etag, second.etag, "a replaced object has a new ETag");
    assert_eq!(
        store.read_range(&key, 0, 1, Some(&first.etag)).await,
        Err(Error::Conflict(Conflict::PackChanged))
    );
    let missing = ObjectKey {
        pack: PackId::random(),
        ..key
    };
    assert_eq!(
        store.read_range(&missing, 0, 1, None).await,
        Err(Error::NotFound)
    );
}

/// Writes `bytes` with the modification time pinned, as a clock too coarse to tell two writes apart would.
fn put_at_a_standstill(
    dir: &Scratch,
    store: &FsStore,
    key: &ObjectKey,
    bytes: &[u8],
) -> ObjectHead {
    let mut writer = store.writer(key).expect("writer");
    writer.write_all(bytes).expect("write");
    let tmp = dir.0.join("objects/tmp");
    let mut files = std::fs::read_dir(&tmp).expect("tmp");
    let file = files
        .next()
        .expect("a temporary file")
        .expect("entry")
        .path();
    assert!(files.next().is_none(), "one writer, one temporary file");
    let pinned = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(file)
        .expect("open")
        .set_modified(pinned)
        .expect("pin the time");
    writer.finish(None).expect("finish")
}

#[tokio::test]
async fn every_replace_of_the_same_size_gets_a_new_etag_whatever_the_clock() {
    let dir = Scratch::new();
    let store = open(&dir);
    let key = key(CacheId::random());
    let mut etags: Vec<String> = Vec::new();
    for round in 0..200u8 {
        let head = put_at_a_standstill(&dir, &store, &key, &[round; 4096]);
        assert_eq!(
            store.head(&key).await.expect("head").as_ref(),
            Some(&head),
            "a head agrees with what the write returned"
        );
        for older in &etags {
            assert_ne!(older, &head.etag, "round {round} reused an ETag");
            assert_eq!(
                store.read_range(&key, 0, 1, Some(older)).await,
                Err(Error::Conflict(Conflict::PackChanged)),
                "round {round}: an older ETag still matches"
            );
        }
        etags.push(head.etag);
    }
}

#[tokio::test]
async fn an_object_that_is_not_replaced_keeps_its_etag_across_reads_and_a_restart() {
    let dir = Scratch::new();
    let store = open(&dir);
    let key = key(CacheId::random());
    let head = store.put(&key, vec![7; 1000]).await.expect("put");
    for _ in 0..3 {
        store
            .read_range(&key, 10, 20, Some(&head.etag))
            .await
            .expect("range");
        assert_eq!(store.head(&key).await.expect("head"), Some(head.clone()));
    }
    let reopened = open(&dir);
    assert_eq!(reopened.head(&key).await.expect("head"), Some(head.clone()));
    let replaced = reopened.put(&key, vec![8; 1000]).await.expect("replace");
    assert_ne!(replaced.etag, head.etag);
    assert_eq!(
        store.head(&key).await.expect("head"),
        Some(replaced),
        "another handle sees the replacement"
    );
}

/// The file that holds `key` in the store at `dir`.
fn file_of(dir: &Scratch, key: &ObjectKey) -> PathBuf {
    let pack = key.pack.to_string();
    dir.0
        .join("objects/v1")
        .join(key.cache.to_string())
        .join(&pack[..2])
        .join(pack)
}

#[tokio::test]
async fn a_copy_of_an_object_without_its_file_attributes_keeps_its_etag() {
    let dir = Scratch::new();
    let store = open(&dir);
    let key = key(CacheId::random());
    let bytes: Vec<u8> = (0..5000u32).map(|n| (n % 251) as u8).collect();
    let head = store.put(&key, bytes.clone()).await.expect("put");
    let file = file_of(&dir, &key);
    std::fs::remove_file(&file).expect("remove");
    std::fs::write(&file, &bytes).expect("a plain copy");
    let reopened = open(&dir);
    assert_eq!(reopened.head(&key).await.expect("head"), Some(head.clone()));
    assert_eq!(
        reopened
            .read_range(&key, 0, 4, Some(&head.etag))
            .await
            .expect("range"),
        bytes[..4]
    );
}

#[tokio::test]
async fn bytes_changed_in_place_change_the_etag() {
    use std::io::Write;
    let dir = Scratch::new();
    let store = open(&dir);
    let key = key(CacheId::random());
    let head = store.put(&key, vec![1; 4096]).await.expect("put");
    let mut file = std::fs::File::options()
        .write(true)
        .open(file_of(&dir, &key))
        .expect("open");
    file.write_all(&[2]).expect("change a byte");
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))
        .expect("set the time");
    drop(file);
    let changed = store.head(&key).await.expect("head").expect("present");
    assert_ne!(
        changed.etag, head.etag,
        "the old ETag still describes new bytes"
    );
    assert_eq!(
        store.read_range(&key, 0, 1, Some(&head.etag)).await,
        Err(Error::Conflict(Conflict::PackChanged))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn under_concurrent_replaces_an_etag_only_ever_comes_with_its_own_bytes() {
    const WRITERS: u16 = 4;
    const ROUNDS: u16 = 50;
    let dir = Scratch::new();
    let store = open(&dir);
    let key = key(CacheId::random());
    let first = store.put(&key, vec![0; 8192]).await.expect("put");
    let mut tags = std::collections::HashMap::from([(first.etag, [0, 0])]);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut writers = tokio::task::JoinSet::new();
    for writer in 0..WRITERS {
        let store = store.clone();
        writers.spawn(async move {
            let mut written = Vec::new();
            for round in 0..ROUNDS {
                let tag = (writer * ROUNDS + round + 1).to_be_bytes();
                let head = store.put(&key, tag.repeat(4096)).await.expect("put");
                written.push((head.etag, tag));
            }
            written
        });
    }
    let mut readers = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let (store, done) = (store.clone(), done.clone());
        readers.spawn(async move {
            let mut seen = Vec::new();
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                let head = store.head(&key).await.expect("head").expect("present");
                match store.read_range(&key, 0, head.size, Some(&head.etag)).await {
                    Ok(bytes) => seen.push((head.etag, bytes)),
                    Err(Error::Conflict(Conflict::PackChanged)) => {}
                    Err(other) => panic!("read: {other:?}"),
                }
            }
            seen
        });
    }
    while let Some(written) = writers.join_next().await {
        for (etag, tag) in written.expect("writer") {
            assert!(tags.insert(etag, tag).is_none(), "two writes share an ETag");
        }
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut checked = 0;
    while let Some(seen) = readers.join_next().await {
        for (etag, bytes) in seen.expect("reader") {
            let tag = tags.get(&etag).expect("an ETag no write returned");
            assert_eq!(bytes, tag.repeat(4096), "ETag {etag} came with other bytes");
            checked += 1;
        }
    }
    assert!(checked > 0, "no read saw a replacement");
}

#[tokio::test]
async fn deleting_is_idempotent_and_listing_is_per_cache_and_ignores_foreign_files() {
    let dir = Scratch::new();
    let store = open(&dir);
    let (cache, other) = (CacheId::random(), CacheId::random());
    let (mine, theirs) = (key(cache), key(other));
    store.put(&mine, vec![1; 10]).await.expect("put");
    store.put(&theirs, vec![1; 20]).await.expect("put");
    let shard = dir.0.join("objects/v1").join(cache.to_string()).join("zz");
    std::fs::create_dir_all(&shard).expect("shard");
    std::fs::write(shard.join("not-a-pack"), b"x").expect("foreign");

    let listed = store.list(&cache).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].pack, listed[0].size), (mine.pack, 10));
    store.delete(&mine).await.expect("delete");
    store.delete(&mine).await.expect("deleting again succeeds");
    assert!(store.head(&mine).await.expect("head").is_none());
    assert_eq!(store.list(&other).await.expect("list").len(), 1);
}

struct Parsed {
    expires_at: i64,
    signature: String,
}

fn parse(url: &str) -> Parsed {
    let query = url.split_once('?').expect("query").1;
    let mut parsed = Parsed {
        expires_at: 0,
        signature: String::new(),
    };
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("exp", v)) => parsed.expires_at = v.parse().expect("exp"),
            Some(("sig", v)) => parsed.signature = v.to_owned(),
            _ => {}
        }
    }
    parsed
}

fn request<'a>(
    method: &'a str,
    key: &ObjectKey,
    parsed: &'a Parsed,
    length: Option<u64>,
) -> UrlRequest<'a> {
    UrlRequest {
        method,
        cache: key.cache,
        pack: key.pack,
        expires_at: parsed.expires_at,
        content_length: length,
        signature: &parsed.signature,
    }
}

#[tokio::test]
async fn signed_urls_bind_method_path_expiry_and_length() {
    let dir = Scratch::new();
    let store = open(&dir);
    let key = key(CacheId::random());
    let put = store
        .put_url(&key, 1234, None, 5_000)
        .await
        .expect("put url");
    assert!(put.url.starts_with(&format!(
        "http://cache.test/v1/caches/{}/objects/{}?",
        key.cache, key.pack
    )));
    assert_eq!(put.method, "PUT");
    assert_eq!(
        put.headers,
        vec![("content-length".to_owned(), "1234".to_owned())]
    );
    let parsed = parse(&put.url);

    assert_eq!(
        store.verify_url(&request("PUT", &key, &parsed, Some(1234)), 4_999),
        Ok(())
    );
    assert_eq!(
        store.verify_url(&request("PUT", &key, &parsed, Some(1234)), 5_000),
        Err(UrlRejection::Expired)
    );
    assert_eq!(
        store.verify_url(&request("PUT", &key, &parsed, Some(1235)), 100),
        Err(UrlRejection::BadSignature),
        "length"
    );
    assert_eq!(
        store.verify_url(&request("GET", &key, &parsed, Some(1234)), 100),
        Err(UrlRejection::BadSignature),
        "method"
    );
    assert_eq!(
        store.verify_url(&request("PUT", &key, &parsed, None), 100),
        Err(UrlRejection::BadSignature),
        "no length"
    );
    let other_pack = ObjectKey {
        pack: PackId::random(),
        ..key
    };
    assert_eq!(
        store.verify_url(&request("PUT", &other_pack, &parsed, Some(1234)), 100),
        Err(UrlRejection::BadSignature),
        "pack"
    );
    let later = Parsed {
        expires_at: parsed.expires_at + 1,
        signature: parsed.signature.clone(),
    };
    assert_eq!(
        store.verify_url(&request("PUT", &key, &later, Some(1234)), 100),
        Err(UrlRejection::BadSignature),
        "expiry"
    );
    for tampered in ["", "AAAA", "not base64!", &format!("{}A", parsed.signature)] {
        let bad = Parsed {
            expires_at: parsed.expires_at,
            signature: tampered.to_owned(),
        };
        assert_eq!(
            store.verify_url(&request("PUT", &key, &bad, Some(1234)), 100),
            Err(UrlRejection::BadSignature)
        );
    }

    let get = store.get_url(&key, 5_000).await.expect("get url");
    assert_eq!(get.method, "GET");
    let get_parsed = parse(&get.url);
    assert_eq!(
        store.verify_url(&request("GET", &key, &get_parsed, None), 100),
        Ok(())
    );
    assert_eq!(
        store.verify_url(&request("PUT", &key, &get_parsed, None), 100),
        Err(UrlRejection::BadSignature)
    );
}

#[tokio::test]
async fn during_key_rotation_both_keys_verify_and_only_the_first_signs() {
    let dir = Scratch::new();
    let old = FsStore::open(dir.0.join("a"), "http://cache.test", vec![[1; 32]]).expect("old");
    let rotating = FsStore::open(dir.0.join("b"), "http://cache.test", vec![[2; 32], [1; 32]])
        .expect("rotating");
    let retired =
        FsStore::open(dir.0.join("c"), "http://cache.test", vec![[2; 32]]).expect("retired");
    let key = key(CacheId::random());
    let signed_by_old = parse(&old.get_url(&key, 5_000).await.expect("url").url);
    let signed_by_new = parse(&rotating.get_url(&key, 5_000).await.expect("url").url);

    assert_eq!(
        rotating.verify_url(&request("GET", &key, &signed_by_old, None), 100),
        Ok(())
    );
    assert_eq!(
        rotating.verify_url(&request("GET", &key, &signed_by_new, None), 100),
        Ok(())
    );
    assert_eq!(
        retired.verify_url(&request("GET", &key, &signed_by_new, None), 100),
        Ok(())
    );
    assert_eq!(
        retired.verify_url(&request("GET", &key, &signed_by_old, None), 100),
        Err(UrlRejection::BadSignature),
        "a retired key no longer verifies"
    );
}

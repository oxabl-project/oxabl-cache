//! The pack half of `migrate`: each live pack is read from the source store, checked
//! against the metadata and against its own footer, and written to the target store.

use crate::store::AnyStore;
use oxabl_cache_core::ids::{CacheId, PackId, SegmentId};
use oxabl_cache_core::ports::migration::{Batch, Cell};
use oxabl_cache_core::ports::{ObjectKey, ObjectStore};
use oxabl_cache_format as format;
use oxabl_cache_format::pack_header::PACK_HEADER_LEN;
use oxabl_cache_format::pack_trailer::PACK_TRAILER_LEN;
use oxabl_cache_meta_sqlite::SqliteExport;
use std::collections::BTreeMap;

const BATCH_ROWS: usize = 1_000;
const LIVE: i64 = 1;

/// A segment the metadata records as live.
#[derive(Debug)]
struct Recorded {
    id: SegmentId,
    offset: u64,
    length: u32,
}

/// A committed pack and what the metadata says it holds.
#[derive(Debug)]
pub(crate) struct LivePack {
    key: ObjectKey,
    size: u64,
    segments: Vec<Recorded>,
}

/// What a pack phase moved.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Moved {
    pub packs: u64,
    pub bytes: u64,
}

fn column(batch: &Batch, name: &str) -> Result<usize, String> {
    batch
        .columns
        .iter()
        .position(|column| column == name)
        .ok_or_else(|| format!("the source has no column {name}"))
}

fn bytes(cell: &Cell) -> Result<&[u8], String> {
    match cell {
        Cell::Bytes(bytes) => Ok(bytes),
        _ => Err("a byte column holds another type".to_owned()),
    }
}

fn int(cell: &Cell) -> Result<i64, String> {
    match cell {
        Cell::Int(value) => Ok(*value),
        _ => Err("an integer column holds another type".to_owned()),
    }
}

fn unsigned(cell: &Cell) -> Result<u64, String> {
    u64::try_from(int(cell)?).map_err(|_| "a negative size or offset".to_owned())
}

fn key_of(cache: &Cell, pack: &Cell) -> Result<ObjectKey, String> {
    Ok(ObjectKey {
        cache: CacheId::from_slice(bytes(cache)?).ok_or("a malformed cache id")?,
        pack: PackId::from_slice(bytes(pack)?).ok_or("a malformed pack id")?,
    })
}

fn read(
    export: &SqliteExport,
    table: &str,
    mut each: impl FnMut(&Batch) -> Result<(), String>,
) -> Result<(), String> {
    let mut failure = None;
    export
        .read(table, BATCH_ROWS, &mut |batch| {
            each(&batch).map_err(|message| {
                failure = Some(message);
                oxabl_cache_core::Error::internal("stopped")
            })
        })
        .map_err(|error| failure.unwrap_or_else(|| format!("reading {table}: {error}")))
}

/// The committed packs of the snapshot, with their live segments.
pub(crate) fn live_packs(export: &SqliteExport) -> Result<Vec<LivePack>, String> {
    let mut packs: BTreeMap<(CacheId, PackId), LivePack> = BTreeMap::new();
    read(export, "packs", |batch| {
        let (cache, pack) = (column(batch, "cache_id")?, column(batch, "pack_id")?);
        let (size, state) = (column(batch, "size")?, column(batch, "state")?);
        for row in &batch.rows {
            if int(&row[state])? == LIVE {
                let key = key_of(&row[cache], &row[pack])?;
                packs.insert(
                    (key.cache, key.pack),
                    LivePack {
                        key,
                        size: unsigned(&row[size])?,
                        segments: Vec::new(),
                    },
                );
            }
        }
        Ok(())
    })?;
    read(export, "segments", |batch| {
        let (cache, pack) = (column(batch, "cache_id")?, column(batch, "pack_id")?);
        let (id, dead) = (column(batch, "segment_id")?, column(batch, "dead_since")?);
        let (offset, length) = (column(batch, "byte_offset")?, column(batch, "byte_length")?);
        for row in &batch.rows {
            if !matches!(row[dead], Cell::Null) {
                continue;
            }
            let key = key_of(&row[cache], &row[pack])?;
            if let Some(live) = packs.get_mut(&(key.cache, key.pack)) {
                live.segments.push(Recorded {
                    id: SegmentId::from_slice(bytes(&row[id])?).ok_or("a malformed segment id")?,
                    offset: unsigned(&row[offset])?,
                    length: u32::try_from(unsigned(&row[length])?)
                        .map_err(|_| "a segment longer than a pack".to_owned())?,
                });
            }
        }
        Ok(())
    })?;
    Ok(packs.into_values().collect())
}

fn hash(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

fn slice(data: &[u8], offset: u64, length: u64) -> Option<&[u8]> {
    let start = usize::try_from(offset).ok()?;
    let end = start.checked_add(usize::try_from(length).ok()?)?;
    data.get(start..end)
}

/// Checks the bytes of a pack against the metadata and its own framing: the declared size, the
/// header's cache, the footer's digest, the hash of every segment the footer lists, and that
/// every live segment the metadata records sits where the footer puts it.
fn verify(data: &[u8], pack: &LivePack) -> Result<(), String> {
    let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
    if len != pack.size {
        return Err(format!(
            "the object holds {len} bytes, the metadata records {}",
            pack.size
        ));
    }
    let framing = |error: format::FormatError| format!("malformed pack: {error}");
    let header = slice(data, 0, PACK_HEADER_LEN as u64).ok_or("shorter than a pack header")?;
    let header = format::PackHeader::decode(header).map_err(framing)?;
    if header.cache_id.0 != *pack.key.cache.as_bytes() {
        return Err("the header names another cache".to_owned());
    }
    let trailer_at = len
        .checked_sub(PACK_TRAILER_LEN as u64)
        .ok_or("shorter than a pack trailer")?;
    let trailer = slice(data, trailer_at, PACK_TRAILER_LEN as u64).ok_or("no trailer")?;
    let trailer = format::PackTrailer::decode(trailer, len).map_err(framing)?;
    let footer = slice(
        data,
        trailer.footer_offset,
        trailer_at - trailer.footer_offset,
    )
    .ok_or("the footer lies outside the pack")?;
    let footer = format::PackFooter::decode(footer, &trailer, hash).map_err(framing)?;
    for segment in &footer.segments {
        let bytes = slice(data, segment.offset, u64::from(segment.length))
            .ok_or("a segment lies outside the pack")?;
        if hash(bytes) != segment.segment_id.0 {
            return Err(format!(
                "segment {} at offset {} does not hash to its id",
                SegmentId::from_bytes(segment.segment_id.0),
                segment.offset
            ));
        }
    }
    for recorded in &pack.segments {
        let listed = footer.segments.iter().any(|segment| {
            segment.segment_id.0 == *recorded.id.as_bytes()
                && segment.offset == recorded.offset
                && segment.length == recorded.length
        });
        if !listed {
            return Err(format!(
                "live segment {} is not where the metadata puts it",
                recorded.id
            ));
        }
    }
    Ok(())
}

/// Verifies every pack in `source`. With a `target` store, writes each to it and reads the
/// bytes back; without one, the stores are the same and the packs stay where they are.
pub(crate) async fn move_packs(
    packs: &[LivePack],
    source: &AnyStore,
    target: Option<&AnyStore>,
) -> Result<Moved, String> {
    let mut moved = Moved::default();
    for pack in packs {
        let name = format!("pack {} of cache {}", pack.key.pack, pack.key.cache);
        let data = match source.head(&pack.key).await {
            Ok(Some(_)) => source
                .read_range(&pack.key, 0, pack.size, None)
                .await
                .map_err(|error| format!("{name}: {error}"))?,
            Ok(None) => return Err(format!("{name}: the object is missing from the source")),
            Err(error) => return Err(format!("{name}: {error}")),
        };
        verify(&data, pack).map_err(|reason| format!("{name}: {reason}"))?;
        if let Some(target) = target {
            target
                .put(&pack.key, data.clone())
                .await
                .map_err(|error| format!("{name}: writing the target: {error}"))?;
            let stored = target
                .read_range(&pack.key, 0, pack.size, None)
                .await
                .map_err(|error| format!("{name}: reading the target back: {error}"))?;
            if stored != data {
                return Err(format!(
                    "{name}: the target holds other bytes than were written"
                ));
            }
        }
        moved.packs += 1;
        moved.bytes += pack.size;
    }
    Ok(moved)
}

use std::io::{Cursor, Read};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use oxabl_cache_crypto::{
    Dictionary, EntryFields, Error, FileEntry, SealOptions, seal_record, seal_segment,
};
use oxabl_cache_format::{LookupId, Namespace, SegmentId};

use super::read_files::read_verified;
use super::types::{ArtifactFileRef, StoreFailure, StoreOutcome};
use crate::cache::{Inner, RosterView};
use crate::compile_key::CompileKey;

/// What sealing one artifact set needs besides the files.
pub(crate) struct SealContext {
    pub inner: Arc<Inner>,
    pub view: Arc<RosterView>,
    pub namespace: Namespace,
    pub dictionary: Option<Arc<Dictionary>>,
}

/// One set, sealed: its segment, its signed record and where they go.
pub(crate) struct Sealed {
    pub slot: usize,
    pub lookup_id: LookupId,
    pub segment: Vec<u8>,
    pub segment_id: SegmentId,
    pub dict_id: Option<SegmentId>,
    pub record: Vec<u8>,
}

fn failed(error: &Error) -> StoreOutcome {
    StoreOutcome::Failed(match error {
        Error::Limit(_) => StoreFailure::TooLarge,
        _ => StoreFailure::Rejected,
    })
}

/// Reads the files, checking each against its reference, compresses and seals them into one
/// segment, and signs the record. The bytes compressed are exactly the bytes checked.
pub(crate) fn seal_set(
    context: &SealContext,
    slot: usize,
    key: CompileKey,
    files: &[ArtifactFileRef],
) -> Result<Sealed, StoreOutcome> {
    let mut stream: Box<dyn Read> = Box::new(std::io::empty());
    for file in files {
        let bytes = read_verified(file).map_err(|f| f.outcome())?;
        stream = Box::new(stream.chain(Cursor::new(bytes)));
    }
    let inner = &context.inner;
    let options = SealOptions {
        level: inner.zstd_level,
        dictionary: context.dictionary.as_deref(),
    };
    let mut segment = Vec::new();
    let seal = seal_segment(
        &context.view.segment,
        stream,
        &options,
        &inner.limits,
        &mut segment,
    )
    .map_err(|e| failed(&e))?;
    let lookup_id = context.view.lookup.lookup_id(&key.0);
    let dict_id = context.dictionary.as_ref().map(|d| d.id());
    let fields = EntryFields {
        segment_id: seal.id,
        segment_len: u32::try_from(seal.sealed_len)
            .map_err(|_| StoreOutcome::Failed(StoreFailure::TooLarge))?,
        dict_id,
        plain_len: seal.plain_len,
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
        roster_version: context.view.roster.version(),
        files: files
            .iter()
            .map(|f| FileEntry {
                path: f.path.as_str().to_owned(),
                size: f.size,
            })
            .collect(),
    };
    let record = seal_record(
        &context.view.record,
        &oxabl_cache_crypto::RecordContext {
            cache_id: inner.pins.cache,
            epoch: context.view.epoch(),
            namespace: context.namespace,
            lookup_id,
        },
        &fields,
        &inner.identity,
        &inner.limits,
    )
    .map_err(|e| failed(&e))?;
    Ok(Sealed {
        slot,
        lookup_id,
        segment,
        segment_id: seal.id,
        dict_id,
        record,
    })
}

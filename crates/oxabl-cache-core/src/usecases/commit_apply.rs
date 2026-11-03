//! The transaction of a commit: decide every entry, then record everything at once.

use crate::domain::entry::{Entry, EntrySpec, EntryStatus};
use crate::domain::namespace::Namespace;
use crate::domain::pack::PackState;
use crate::domain::segment::{Segment, SegmentKind, SegmentSpec};
use crate::error::{Conflict, Error, Result};
use crate::ids::{CacheId, PackId, PrincipalId, SegmentId, TenantId};
use crate::limits::RECORD_BYTES;
use crate::ports::MetaTx;
use crate::ports::metadata::RecordedCommit;
use crate::time::day_of;
use std::collections::{HashMap, HashSet};

/// Everything the transaction needs from the request and the verified object.
pub(crate) struct CommitInput {
    pub tenant: TenantId,
    pub cache: CacheId,
    pub pack: PackId,
    pub caller: PrincipalId,
    pub namespace: Namespace,
    pub digest: [u8; 32],
    pub etag: String,
    pub segments: Vec<SegmentSpec>,
    pub entries: Vec<EntrySpec>,
    pub now: i64,
}

/// The statuses to return and whether this call recorded them.
pub(crate) struct CommitOutput {
    pub statuses: Vec<EntryStatus>,
    pub recorded: bool,
    pub pack_size: u64,
}

/// Answers a repeat from the record: the same statuses for the same body, `pack_closed` otherwise.
pub(crate) fn answer_repeat(
    statuses: &[u8],
    recorded_digest: &[u8; 32],
    digest: &[u8; 32],
) -> Result<Vec<EntryStatus>> {
    if recorded_digest != digest {
        return Err(Error::Conflict(Conflict::PackClosed));
    }
    statuses
        .iter()
        .map(|code| EntryStatus::from_code(*code))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| Error::internal("recorded commit status is unknown"))
}

pub(crate) fn apply_commit(tx: &mut dyn MetaTx, input: CommitInput) -> Result<CommitOutput> {
    let cache = tx
        .cache(&input.tenant, &input.cache)?
        .ok_or(Error::NotFound)?;
    let pack = tx.pack(&input.cache, &input.pack)?.ok_or(Error::NotFound)?;
    if pack.uploader != input.caller {
        return Err(Error::Forbidden("not the uploader of this pack"));
    }
    if let (Some(digest), Some(statuses)) = (&pack.commit_digest, &pack.commit_statuses) {
        return Ok(CommitOutput {
            statuses: answer_repeat(statuses, digest, &input.digest)?,
            recorded: false,
            pack_size: pack.size,
        });
    }
    if pack.state != PackState::Pending {
        return Err(Error::Conflict(Conflict::PackClosed));
    }
    if pack.namespace != input.namespace.key() {
        return Err(Error::bad_request("the pack belongs to another namespace"));
    }

    let ids: Vec<SegmentId> = input.segments.iter().map(|s| s.id).collect();
    let recorded = tx.existing_segment_ids(&input.cache, &ids)?;
    let mut table_count: HashMap<SegmentId, u32> = HashMap::new();
    for id in &ids {
        *table_count.entry(*id).or_insert(0) += 1;
    }
    let usable = |id: &SegmentId| !recorded.contains(id) && table_count[id] == 1;

    let wanted: Vec<SegmentId> = input.entries.iter().filter_map(|e| e.dict_id).collect();
    let live_dictionaries = tx.live_dictionaries(&input.cache, &wanted)?;

    let mut statuses: Vec<Option<EntryStatus>> = vec![None; input.entries.len()];
    let mut naming: HashMap<usize, Vec<usize>> = HashMap::new();
    for (position, entry) in input.entries.iter().enumerate() {
        let segment = usize::try_from(entry.segment_index)
            .ok()
            .and_then(|index| input.segments.get(index).map(|spec| (index, spec)));
        let well_formed = entry.header_ok
            && !entry.record.is_empty()
            && entry.record.len() <= RECORD_BYTES
            && entry.epoch == cache.epoch;
        match segment {
            Some((index, spec))
                if well_formed && spec.kind == SegmentKind::Artifact && usable(&spec.id) =>
            {
                naming.entry(index).or_default().push(position);
            }
            _ => statuses[position] = Some(EntryStatus::Invalid),
        }
    }
    for positions in naming.values().filter(|positions| positions.len() > 1) {
        for position in positions {
            statuses[*position] = Some(EntryStatus::Invalid);
        }
    }
    for (position, entry) in input.entries.iter().enumerate() {
        let missing = entry
            .dict_id
            .is_some_and(|id| !live_dictionaries.contains_key(&id));
        if statuses[position].is_none() && missing {
            statuses[position] = Some(EntryStatus::MissingDictionary);
        }
    }

    let mut credited: HashSet<SegmentId> = HashSet::new();
    let mut inserted: HashSet<SegmentId> = HashSet::new();
    let mut fresh: Vec<(usize, Segment, bool)> = Vec::new();
    for (index, spec) in input.segments.iter().enumerate() {
        if !usable(&spec.id) {
            continue;
        }
        let candidate = match spec.kind {
            SegmentKind::Dictionary => true,
            SegmentKind::Artifact => naming
                .get(&index)
                .is_some_and(|positions| positions.len() == 1 && statuses[positions[0]].is_none()),
        };
        let segment = Segment {
            cache: input.cache,
            id: spec.id,
            pack: input.pack,
            offset: spec.offset,
            length: spec.length,
            kind: spec.kind,
            created_at: input.now,
            dead_since: (!candidate).then_some(input.now),
        };
        fresh.push((index, segment, candidate));
    }
    let rows: Vec<Segment> = fresh
        .iter()
        .map(|(_, segment, _)| segment.clone())
        .collect();
    let results = tx.insert_segments(&rows)?;
    if results.len() != rows.len() {
        return Err(Error::internal("a segment batch was answered in part"));
    }
    for ((index, segment, candidate), new) in fresh.into_iter().zip(results) {
        if !new {
            for position in naming.get(&index).into_iter().flatten() {
                statuses[*position] = Some(EntryStatus::Invalid);
            }
            continue;
        }
        inserted.insert(segment.id);
        if candidate {
            credited.insert(segment.id);
        }
    }

    let today = day_of(input.now);
    let mut contenders: Vec<(usize, Entry)> = Vec::new();
    for (position, entry) in input.entries.iter().enumerate() {
        if statuses[position].is_some() {
            continue;
        }
        let spec = &input.segments[entry.segment_index as usize];
        if !inserted.contains(&spec.id) {
            statuses[position] = Some(EntryStatus::Invalid);
            continue;
        }
        contenders.push((
            position,
            Entry {
                cache: input.cache,
                namespace: input.namespace.key(),
                lookup_id: entry.lookup_id,
                segment_id: spec.id,
                dict_id: entry.dict_id,
                epoch: entry.epoch,
                record: entry.record.clone(),
                created_at: input.now,
            },
        ));
    }
    let rows: Vec<Entry> = contenders.iter().map(|(_, row)| row.clone()).collect();
    let results = tx.insert_entries(&rows, today)?;
    if results.len() != rows.len() {
        return Err(Error::internal("an entry batch was answered in part"));
    }
    for ((position, row), won) in contenders.into_iter().zip(results) {
        if won {
            statuses[position] = Some(EntryStatus::Committed);
        } else {
            statuses[position] = Some(EntryStatus::Exists);
            tx.kill_segment(&input.cache, &row.segment_id, input.now)?;
            credited.remove(&row.segment_id);
        }
    }

    let live_bytes: u64 = input
        .segments
        .iter()
        .filter(|spec| credited.contains(&spec.id))
        .map(|spec| u64::from(spec.length))
        .sum();
    let statuses: Vec<EntryStatus> = statuses
        .into_iter()
        .map(|status| status.unwrap_or(EntryStatus::Invalid))
        .collect();
    let codes: Vec<u8> = statuses.iter().map(|status| status.code()).collect();
    let committed = tx.record_commit(
        &input.cache,
        &input.pack,
        &RecordedCommit {
            etag: &input.etag,
            live_bytes,
            digest: input.digest,
            statuses: &codes,
            dead_since: (live_bytes == 0).then_some(input.now),
        },
    )?;
    if !committed {
        return Err(Error::internal(
            "pack left pending state during its own commit",
        ));
    }
    let live = i64::try_from(live_bytes).map_err(|_| Error::internal("live bytes overflow"))?;
    let pending = i64::try_from(pack.size).map_err(|_| Error::internal("pack size overflow"))?;
    tx.adjust_cache_bytes(&input.cache, live, -pending)?;
    Ok(CommitOutput {
        statuses,
        recorded: true,
        pack_size: pack.size,
    })
}

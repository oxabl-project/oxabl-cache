/// Neighbouring segments closer than this are read with one request.
pub(crate) const COALESCE_GAP: u64 = 1024 * 1024;
/// Largest read of one request.
pub(crate) const MAX_READ: u64 = 8 * 1024 * 1024;

/// A segment to read: its pack, its place in the caller's list, and where it sits.
#[derive(Clone, Copy)]
pub(crate) struct Wanted {
    pub pack_idx: usize,
    pub item: usize,
    pub offset: u64,
    pub length: u32,
}

/// One contiguous read of a pack that serves several segments.
pub(crate) struct Range {
    pub pack_idx: usize,
    pub start: u64,
    pub end: u64,
    pub members: Vec<Wanted>,
}

impl Range {
    pub fn len(&self) -> u64 {
        self.end - self.start
    }
}

/// Groups segments by pack and merges neighbours with gaps of 1 MiB or less into reads of at
/// most 8 MiB; a segment larger than that is a read of its own.
pub(crate) fn coalesce(mut wanted: Vec<Wanted>) -> Vec<Range> {
    wanted.sort_by_key(|w| (w.pack_idx, w.offset));
    let mut ranges: Vec<Range> = Vec::new();
    for segment in wanted {
        let end = segment.offset + u64::from(segment.length);
        if let Some(open) = ranges.last_mut()
            && open.pack_idx == segment.pack_idx
            && segment.offset.saturating_sub(open.end) <= COALESCE_GAP
            && end.max(open.end) - open.start <= MAX_READ
        {
            open.end = open.end.max(end);
            open.members.push(segment);
            continue;
        }
        ranges.push(Range {
            pack_idx: segment.pack_idx,
            start: segment.offset,
            end,
            members: vec![segment],
        });
    }
    ranges
}

//! `OXLR` lookup response.

use std::collections::HashSet;

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::{read_header, write_header};
use crate::ids::{PackId, SegmentId};
use crate::location::check_range;
use crate::lookup_request::{LookupFlags, LookupShape};
use crate::namespace::Namespace;
use crate::writer::{Writer, len32};

const MAGIC: [u8; 4] = *b"OXLR";

/// A pack the response refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsePack<'a> {
    pub pack_id: PackId,
    /// Presigned GET URL; empty unless the request asked for URLs.
    pub url: &'a str,
    pub expires_at: u64,
}

/// A hit's record and where its segment lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitRecord<'a> {
    pub pack_idx: u32,
    pub offset: u64,
    pub length: u32,
    /// Sealed record bytes, opaque at this layer.
    pub record: &'a [u8],
}

/// One hit of one request id in one namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupHit<'a> {
    /// Position of the id in the request.
    pub index: u32,
    pub ns: Namespace,
    /// Present exactly when the request asked for records.
    pub record: Option<HitRecord<'a>>,
}

/// A dictionary segment named by a returned entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseDictionary {
    pub dict_id: SegmentId,
    pub pack_idx: u32,
    pub offset: u64,
    pub length: u32,
}

/// A lookup response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupResponse<'a> {
    pub flags: LookupFlags,
    pub packs: Vec<ResponsePack<'a>>,
    pub hits: Vec<LookupHit<'a>>,
    pub dictionaries: Vec<ResponseDictionary>,
}

const PACK_MIN: usize = 16 + 2 + 8;
const HIT_PRESENCE: usize = 5;
const HIT_RECORD: usize = HIT_PRESENCE + 4 + 8 + 4 + 2;
const DICT_LEN: usize = 32 + 4 + 8 + 4;

fn max_hits(shape: &LookupShape) -> usize {
    (shape.count as usize).saturating_mul(shape.namespaces.len())
}

impl<'a> LookupResponse<'a> {
    pub fn encode(&self, shape: &LookupShape) -> Result<Vec<u8>> {
        self.check(shape)?;
        let mut w = Writer::default();
        write_header(&mut w, MAGIC, self.flags.to_bits());
        w.u16(0);
        w.u32(len32(self.packs.len(), "response packs")?);
        for p in &self.packs {
            w.bytes(&p.pack_id.0);
            w.str16(p.url)?;
            w.u64(p.expires_at);
        }
        w.u32(len32(self.hits.len(), "response hits")?);
        for h in &self.hits {
            w.u32(h.index);
            w.u8(h.ns.to_u8());
            if let Some(r) = &h.record {
                w.u32(r.pack_idx);
                w.u64(r.offset);
                w.u32(r.length);
                w.record(r.record)?;
            }
        }
        w.u32(len32(self.dictionaries.len(), "response dictionaries")?);
        for d in &self.dictionaries {
            w.bytes(&d.dict_id.0);
            w.u32(d.pack_idx);
            w.u64(d.offset);
            w.u32(d.length);
        }
        Ok(w.into_vec())
    }

    /// Decodes a response to a request of the given shape.
    pub fn decode(bytes: &'a [u8], shape: &LookupShape) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        let flags = LookupFlags::from_bits(read_header(&mut c, MAGIC, 0b11)?);
        if flags != shape.flags {
            return Err(FormatError::Invalid("flags differ from the request"));
        }
        c.reserved_u16()?;
        let hit_cap = max_hits(shape);
        let pack_cap = hit_cap.saturating_mul(2);
        let pack_count = c.count32(pack_cap, "response packs", PACK_MIN)?;
        let mut packs = Vec::with_capacity(pack_count);
        for _ in 0..pack_count {
            packs.push(ResponsePack {
                pack_id: PackId(c.array()?),
                url: c.str16()?,
                expires_at: c.u64()?,
            });
        }
        let hit_min = if flags.records {
            HIT_RECORD
        } else {
            HIT_PRESENCE
        };
        let hit_count = c.count32(hit_cap, "response hits", hit_min)?;
        let mut hits = Vec::with_capacity(hit_count);
        for _ in 0..hit_count {
            let index = c.u32()?;
            let ns = Namespace::from_u8(c.u8()?)?;
            let record = if flags.records {
                let pack_idx = c.u32()?;
                let offset = c.u64()?;
                let length = c.u32()?;
                Some(HitRecord {
                    pack_idx,
                    offset,
                    length,
                    record: c.record()?,
                })
            } else {
                None
            };
            hits.push(LookupHit { index, ns, record });
        }
        let dict_count = c.count32(hit_cap, "response dictionaries", DICT_LEN)?;
        let mut dictionaries = Vec::with_capacity(dict_count);
        for _ in 0..dict_count {
            dictionaries.push(ResponseDictionary {
                dict_id: SegmentId(c.array()?),
                pack_idx: c.u32()?,
                offset: c.u64()?,
                length: c.u32()?,
            });
        }
        c.finish()?;
        let response = Self {
            flags,
            packs,
            hits,
            dictionaries,
        };
        response.check(shape)?;
        Ok(response)
    }

    fn check(&self, shape: &LookupShape) -> Result<()> {
        if self.flags != shape.flags {
            return Err(FormatError::Invalid("flags differ from the request"));
        }
        let hit_cap = max_hits(shape);
        if self.hits.len() > hit_cap || self.dictionaries.len() > hit_cap {
            return Err(FormatError::LimitExceeded("response hits"));
        }
        if self.packs.len() > hit_cap.saturating_mul(2) {
            return Err(FormatError::LimitExceeded("response packs"));
        }
        if !self.flags.urls && self.packs.iter().any(|p| !p.url.is_empty()) {
            return Err(FormatError::Invalid("url without the urls flag"));
        }
        if self.flags.urls && self.packs.iter().any(|p| p.url.is_empty()) {
            return Err(FormatError::Invalid("missing url"));
        }
        if !self.flags.records && !self.dictionaries.is_empty() {
            return Err(FormatError::Invalid(
                "dictionaries without the records flag",
            ));
        }
        let pack_count = self.packs.len();
        let in_range = |idx: u32| (idx as usize) < pack_count;
        let mut seen = HashSet::with_capacity(self.hits.len());
        for h in &self.hits {
            if h.index >= shape.count || !shape.namespaces.contains(h.ns) {
                return Err(FormatError::Invalid("hit outside the request"));
            }
            if !seen.insert((h.index, h.ns)) {
                return Err(FormatError::Invalid("duplicate hit"));
            }
            match (&h.record, self.flags.records) {
                (Some(r), true) => {
                    if !in_range(r.pack_idx) {
                        return Err(FormatError::Invalid("pack index"));
                    }
                    check_range(r.offset, r.length)?;
                }
                (None, false) => {}
                _ => return Err(FormatError::Invalid("record presence")),
            }
        }
        let mut dicts = HashSet::with_capacity(self.dictionaries.len());
        for d in &self.dictionaries {
            if !in_range(d.pack_idx) {
                return Err(FormatError::Invalid("pack index"));
            }
            check_range(d.offset, d.length)?;
            if !dicts.insert(d.dict_id) {
                return Err(FormatError::Invalid("duplicate dictionary"));
            }
        }
        Ok(())
    }
}

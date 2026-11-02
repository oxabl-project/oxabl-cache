//! Writing pack objects for compaction with the format crate.

use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{AssembledPack, PackAssembler, PackPart, Placement};
use oxabl_cache_core::{Error, Result};
use oxabl_cache_format as format;

/// Lays segments out as an `OXPK` object: header, segments, footer, trailer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FormatAssembler;

fn layout(error: format::FormatError) -> Error {
    Error::internal(format!("pack layout: {error}"))
}

impl PackAssembler for FormatAssembler {
    fn assemble(&self, cache: &CacheId, parts: Vec<PackPart>) -> Result<AssembledPack> {
        let header = format::PackHeader {
            cache_id: format::CacheId(*cache.as_bytes()),
        };
        let mut bytes = header.encode().to_vec();
        let mut placements = Vec::with_capacity(parts.len());
        let mut index = Vec::with_capacity(parts.len());
        for part in &parts {
            let offset = u64::try_from(bytes.len()).map_err(|_| Error::internal("pack offset"))?;
            let length = u32::try_from(part.bytes.len()).map_err(|_| Error::TooLarge)?;
            placements.push(Placement {
                id: part.id,
                offset,
                length,
            });
            index.push(format::PackSegment {
                segment_id: format::SegmentId(*part.id.as_bytes()),
                offset,
                length,
            });
            bytes.extend_from_slice(&part.bytes);
        }
        let footer = format::PackFooter { segments: index };
        let trailer = footer
            .trailer(|data| *blake3::hash(data).as_bytes())
            .map_err(layout)?;
        bytes.extend_from_slice(&footer.encode().map_err(layout)?);
        bytes.extend_from_slice(&trailer.encode().map_err(layout)?);
        Ok(AssembledPack { bytes, placements })
    }
}

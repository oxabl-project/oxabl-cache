//! The pack byte layout of protocol.md, written without the format crate.

use oxabl_cache_core::Result;
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{AssembledPack, PackAssembler, PackPart, Placement};

#[derive(Debug, Clone, Copy)]
pub struct TestAssembler;

impl PackAssembler for TestAssembler {
    fn assemble(&self, cache: &CacheId, parts: Vec<PackPart>) -> Result<AssembledPack> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"OXPK\x01\x00\x00\x00");
        bytes.extend_from_slice(cache.as_bytes());
        let mut placements = Vec::new();
        for part in &parts {
            placements.push(Placement {
                id: part.id,
                offset: bytes.len() as u64,
                length: u32::try_from(part.bytes.len()).expect("test segments are small"),
            });
            bytes.extend_from_slice(&part.bytes);
        }
        let footer_offset = bytes.len() as u64;
        let mut footer = Vec::new();
        for placement in &placements {
            footer.extend_from_slice(placement.id.as_bytes());
            footer.extend_from_slice(&placement.offset.to_be_bytes());
            footer.extend_from_slice(&placement.length.to_be_bytes());
        }
        bytes.extend_from_slice(&footer);
        bytes.extend_from_slice(&u32::try_from(placements.len()).expect("few").to_be_bytes());
        bytes.extend_from_slice(&footer_offset.to_be_bytes());
        bytes.extend_from_slice(blake3::hash(&footer).as_bytes());
        bytes.extend_from_slice(b"OXPF");
        Ok(AssembledPack { bytes, placements })
    }
}

#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::{PackFooter, PackTrailer};
use oxabl_cache_fuzz::blake3;

fuzz_target!(|data: &[u8]| {
    let Some((offset, footer)) = data.split_first_chunk::<8>() else {
        return;
    };
    let trailer = PackTrailer {
        segment_count: (footer.len() / 36) as u32,
        footer_offset: u64::from_be_bytes(*offset),
        footer_digest: blake3(footer),
    };
    if let Ok(value) = PackFooter::decode(footer, &trailer, blake3) {
        let encoded = value.encode().expect("a decoded value encodes");
        assert_eq!(encoded, footer);
        assert_eq!(value.trailer(blake3), Ok(trailer));
        assert_eq!(PackFooter::decode(&encoded, &trailer, blake3), Ok(value));
    }
});

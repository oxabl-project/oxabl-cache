#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::SealedSegment;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = SealedSegment::decode(data) {
        let encoded = value.encode().expect("a decoded value encodes");
        assert_eq!(encoded, data);
        assert_eq!(SealedSegment::decode(&encoded), Ok(value));
    }
});

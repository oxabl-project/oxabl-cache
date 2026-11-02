#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::PackHeader;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = PackHeader::decode(data) {
        let encoded = value.encode();
        assert_eq!(encoded.as_slice(), data);
        assert_eq!(PackHeader::decode(&encoded), Ok(value));
    }
});

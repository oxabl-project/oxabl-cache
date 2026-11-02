#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::LookupRequest;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = LookupRequest::decode(data) {
        let encoded = value.encode().expect("a decoded value encodes");
        assert_eq!(encoded, data);
        assert_eq!(LookupRequest::decode(&encoded), Ok(value));
    }
});

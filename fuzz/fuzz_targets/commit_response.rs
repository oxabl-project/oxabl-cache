#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::CommitResponse;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = CommitResponse::decode(data) {
        let encoded = value.encode().expect("a decoded value encodes");
        assert_eq!(encoded, data);
        assert_eq!(CommitResponse::decode(&encoded), Ok(value));
    }
});

#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::CommitRequest;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = CommitRequest::decode(data) {
        let encoded = value.encode().expect("a decoded value encodes");
        assert_eq!(encoded, data);
        assert_eq!(CommitRequest::decode(&encoded), Ok(value));
    }
});

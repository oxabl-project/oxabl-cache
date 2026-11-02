#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::{EntryPayload, Limits};

fuzz_target!(|data: &[u8]| {
    let limits = Limits::default();
    if let Ok(value) = EntryPayload::decode(data, &limits) {
        let encoded = value.encode(&limits).expect("a decoded value encodes");
        assert_eq!(encoded, data);
        assert_eq!(EntryPayload::decode(&encoded, &limits), Ok(value));
    }
});

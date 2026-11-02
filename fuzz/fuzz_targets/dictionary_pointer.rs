#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::DictionaryPointer;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = DictionaryPointer::decode(data) {
        let encoded = value.encode();
        assert_eq!(encoded, data);
        assert_eq!(DictionaryPointer::decode(&encoded), Ok(value));
    }
});

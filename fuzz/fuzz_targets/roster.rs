#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::Roster;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = Roster::decode(data) {
        let encoded = value.encode().expect("a decoded value encodes");
        assert_eq!(encoded, data);
        assert_eq!(Roster::decode(&encoded), Ok(value));
    }
});

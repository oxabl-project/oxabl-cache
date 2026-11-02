#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::MemberDescriptor;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(value) = MemberDescriptor::decode(text) {
        let encoded = value.encode().expect("a decoded value encodes");
        assert_eq!(encoded, text);
        assert_eq!(MemberDescriptor::decode(&encoded), Ok(value));
    }
});

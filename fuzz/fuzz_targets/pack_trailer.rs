#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::PackTrailer;

fuzz_target!(|data: &[u8]| {
    let Some((size, trailer)) = data.split_first_chunk::<8>() else {
        return;
    };
    let object_size = u64::from_be_bytes(*size);
    if let Ok(value) = PackTrailer::decode(trailer, object_size) {
        let encoded = value.encode().expect("a decoded value encodes");
        assert_eq!(encoded.as_slice(), trailer);
        assert_eq!(PackTrailer::decode(&encoded, object_size), Ok(value));
    }
});

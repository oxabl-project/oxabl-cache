#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::{LookupFlags, LookupResponse, LookupShape, Namespaces};

fuzz_target!(|data: &[u8]| {
    let Some((head, frame)) = data.split_first_chunk::<4>() else {
        return;
    };
    let shape = LookupShape {
        flags: LookupFlags {
            records: head[0] & 1 != 0,
            urls: head[0] & 2 != 0,
        },
        namespaces: Namespaces {
            trusted: head[1] & 1 != 0,
            personal: head[1] & 2 != 0,
        },
        count: u32::from(u16::from_be_bytes([head[2], head[3]])),
    };
    if let Ok(value) = LookupResponse::decode(frame, &shape) {
        let encoded = value.encode(&shape).expect("a decoded value encodes");
        assert_eq!(encoded, frame);
        assert_eq!(LookupResponse::decode(&encoded, &shape), Ok(value));
    }
});

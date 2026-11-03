#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| oxabl_cache_fuzz::crypto::roster(data));

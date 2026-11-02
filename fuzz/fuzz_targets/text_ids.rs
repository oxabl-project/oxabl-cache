#![no_main]

use libfuzzer_sys::fuzz_target;
use oxabl_cache_format::{ApiToken, CacheId, RequestId, TenantId};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(id) = CacheId::from_text(text) {
        assert_eq!(id.to_text(), text);
    }
    if let Ok(id) = TenantId::from_text(text) {
        assert_eq!(id.to_text(), text);
    }
    if let Ok(id) = RequestId::from_text(text) {
        assert_eq!(id.to_text(), text);
    }
    if let Ok(token) = ApiToken::from_text(text) {
        assert_eq!(token.to_text(), text);
    }
});

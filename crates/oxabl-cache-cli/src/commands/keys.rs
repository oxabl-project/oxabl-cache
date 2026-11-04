//! Compile keys as the command line spells them.

use oxabl_cache_client::CompileKey;

use crate::failure::Failure;

/// 64 hex digits, either case.
pub fn parse(text: &str) -> Result<CompileKey, Failure> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return Err(Failure::usage(format!(
            "not a compile key: {text:?} (64 hex digits)"
        )));
    }
    let mut key = [0u8; 32];
    for (slot, pair) in key.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).map_err(|_| Failure::usage("not hex"))?;
        *slot = u8::from_str_radix(pair, 16)
            .map_err(|_| Failure::usage(format!("not a compile key: {text:?} (64 hex digits)")))?;
    }
    Ok(CompileKey(key))
}

pub fn hex(key: &CompileKey) -> String {
    key.0.iter().map(|b| format!("{b:02x}")).collect()
}

/// Padmé length of a plaintext of `len` bytes, or `None` if it does not fit in `u64`.
pub fn padme(len: u64) -> Option<u64> {
    if len < 2 {
        return Some(len);
    }
    let e = len.ilog2();
    let s = e.ilog2() + 1;
    let mask = (1u64 << (e - s)) - 1;
    len.checked_add(mask).map(|v| v & !mask)
}

/// Deterministic generator for synthetic artifact bytes.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }

    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        let mut out = vec![0u8; len];
        self.fill(&mut out);
        out
    }
}

const VOCABULARY: [&str; 12] = [
    "ar/customer.p",
    "INTEGER",
    "CHARACTER",
    "FIND FIRST",
    "BUFFER-COPY",
    "RETURN ERROR",
    "src/common/",
    "DEFINE VARIABLE",
    "ttOrderLine",
    "PROCEDURE",
    "./inc/order.i",
    "Customer not found",
];

/// Bytes that compress like r-code: a binary header, a block of boilerplate every file shares,
/// a string table and some noise.
pub fn rcode_like(rng: &mut Rng, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 64);
    out.extend_from_slice(&[0x56, 0x6b, 0x00, 0x00, 0x0c, 0x08]);
    if len > 8_000 {
        out.extend_from_slice(&boilerplate());
    }
    while out.len() < len {
        match rng.below(10) {
            0..=6 => out.extend_from_slice(VOCABULARY[rng.below(12) as usize].as_bytes()),
            7 => {
                let n = rng.below(24) as usize + 1;
                out.extend_from_slice(&rng.bytes(n));
            }
            _ => out.extend_from_slice(&(rng.next_u64() as u32).to_be_bytes()),
        }
        out.push(0);
    }
    out.truncate(len);
    out
}

fn boilerplate() -> Vec<u8> {
    let mut rng = Rng::new(0x00c0_ffee);
    let mut out = Vec::new();
    while out.len() < 3_000 {
        out.extend_from_slice(VOCABULARY[rng.below(12) as usize].as_bytes());
        out.extend_from_slice(&rng.bytes(3));
    }
    out
}

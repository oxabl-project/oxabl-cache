//! Bech32 (BIP 173 checksum) without the 90-character limit, lowercase only.

use crate::error::{FormatError, Result};
use crate::limits::BECH32_CHARS;

/// HRP of a member identity string. Its payload is secret and is handled by the crypto crate.
pub const IDENTITY_HRP: &str = "oxabl-cache-identity-";

const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const GENERATOR: [u32; 5] = [
    0x3b6a_57b2,
    0x2650_8e6d,
    0x1ea1_19fa,
    0x3d42_33dd,
    0x2a14_62b3,
];

fn polymod(values: impl IntoIterator<Item = u8>) -> u32 {
    let mut chk = 1u32;
    for v in values {
        let top = chk >> 25;
        chk = (chk & 0x01ff_ffff) << 5 ^ u32::from(v);
        for (i, g) in GENERATOR.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

fn hrp_expand(hrp: &str) -> impl Iterator<Item = u8> + '_ {
    hrp.bytes()
        .map(|c| c >> 5)
        .chain(std::iter::once(0))
        .chain(hrp.bytes().map(|c| c & 31))
}

fn to_groups(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity((data.len() * 8).div_ceil(5));
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in data {
        acc = acc << 8 | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push((acc >> bits & 31) as u8);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push((acc << (5 - bits) & 31) as u8);
    }
    out
}

fn from_groups(groups: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(groups.len() * 5 / 8);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &g in groups {
        acc = acc << 5 | u32::from(g);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
        acc &= (1 << bits) - 1;
    }
    if bits >= 5 || acc != 0 {
        return Err(FormatError::Invalid("bech32 padding"));
    }
    Ok(out)
}

fn check_hrp(hrp: &str) -> Result<()> {
    if hrp.is_empty()
        || !hrp
            .bytes()
            .all(|c| (33..=126).contains(&c) && !c.is_ascii_uppercase())
    {
        return Err(FormatError::Invalid("bech32 hrp"));
    }
    Ok(())
}

/// Encodes `data` under `hrp`, which must be printable ASCII without uppercase letters.
pub fn encode(hrp: &str, data: &[u8]) -> Result<String> {
    check_hrp(hrp)?;
    let groups = to_groups(data);
    if hrp.len() + 1 + groups.len() + 6 > BECH32_CHARS {
        return Err(FormatError::LimitExceeded("bech32 length"));
    }
    let chk = polymod(
        hrp_expand(hrp)
            .chain(groups.iter().copied())
            .chain([0u8; 6]),
    ) ^ 1;
    let mut out = String::with_capacity(hrp.len() + 1 + groups.len() + 6);
    out.push_str(hrp);
    out.push('1');
    for g in groups {
        out.push(char::from(CHARSET[usize::from(g)]));
    }
    for i in 0..6 {
        out.push(char::from(CHARSET[(chk >> (5 * (5 - i)) & 31) as usize]));
    }
    Ok(out)
}

/// Decodes a lowercase Bech32 string that must carry `expected_hrp`.
pub fn decode(s: &str, expected_hrp: &str) -> Result<Vec<u8>> {
    if s.len() > BECH32_CHARS {
        return Err(FormatError::LimitExceeded("bech32 length"));
    }
    check_hrp(expected_hrp)?;
    let rest = s
        .strip_prefix(expected_hrp)
        .and_then(|r| r.strip_prefix('1'))
        .ok_or(FormatError::Invalid("bech32 hrp"))?;
    if rest.len() < 6 {
        return Err(FormatError::Invalid("bech32 checksum"));
    }
    let mut groups = Vec::with_capacity(rest.len());
    for c in rest.bytes() {
        let g = CHARSET
            .iter()
            .position(|&x| x == c)
            .ok_or(FormatError::Invalid("bech32 character"))?;
        groups.push(g as u8);
    }
    if polymod(hrp_expand(expected_hrp).chain(groups.iter().copied())) != 1 {
        return Err(FormatError::Invalid("bech32 checksum"));
    }
    groups.truncate(groups.len() - 6);
    from_groups(&groups)
}

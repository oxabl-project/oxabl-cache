//! The text forms of identifiers: lowercase hex and canonical lowercase Crockford base32.

use oxabl_cache_core::ids::{
    CacheId, CredentialId, LookupId, PackId, PrincipalId, SegmentId, TenantId,
};

const CROCKFORD: &str = "0123456789abcdefghjkmnpqrstvwxyz";

#[test]
fn base32_ids_are_one_big_endian_integer_in_26_canonical_characters() {
    let zero = CacheId::from_bytes([0; 16]).to_string();
    assert_eq!(zero, format!("c_{}", "0".repeat(26)));
    let ones = TenantId::from_bytes([0xff; 16]).to_string();
    assert_eq!(
        ones,
        format!("t_7{}", "z".repeat(25)),
        "128 one bits start with the digit 7"
    );
    let one = PrincipalId::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]).to_string();
    assert_eq!(one, format!("p_{}1", "0".repeat(25)));

    for _ in 0..200 {
        let id = CacheId::random();
        let text = id.to_string();
        let body = text.strip_prefix("c_").expect("prefix");
        assert_eq!(body.len(), 26);
        assert!(
            ('0'..='7').contains(&body.chars().next().expect("first")),
            "{text}"
        );
        assert!(body.chars().all(|c| CROCKFORD.contains(c)), "{text}");
        assert_eq!(text.parse::<CacheId>(), Ok(id));
    }
}

#[test]
fn only_the_canonical_spelling_of_an_id_parses() {
    let id = CacheId::random();
    let text = id.to_string();
    assert!(text.to_uppercase().parse::<CacheId>().is_err(), "uppercase");
    assert!(
        text.replacen("c_", "t_", 1).parse::<CacheId>().is_err(),
        "wrong prefix"
    );
    assert!(text[2..].parse::<CacheId>().is_err(), "no prefix");
    assert!(format!("{text}0").parse::<CacheId>().is_err(), "too long");
    assert!(
        text[..text.len() - 1].parse::<CacheId>().is_err(),
        "too short"
    );
    for letter in ['i', 'l', 'o', 'u', '_', '-'] {
        let bad = format!("{}{letter}", &text[..text.len() - 1]);
        assert!(
            bad.parse::<CacheId>().is_err(),
            "{letter} is outside the alphabet"
        );
    }
    let overflowing = format!("c_8{}", "0".repeat(25));
    assert!(
        overflowing.parse::<CacheId>().is_err(),
        "a first digit above 7 does not fit 128 bits"
    );
}

#[test]
fn hex_ids_are_lowercase_and_exact_in_length() {
    let pack = PackId::random();
    assert_eq!(pack.to_string().len(), 32);
    assert_eq!(pack.to_string().parse::<PackId>(), Ok(pack));
    assert!(pack.to_string().to_uppercase().parse::<PackId>().is_err());
    let segment = SegmentId::of_bytes(b"sealed bytes");
    assert_eq!(segment.to_string().len(), 64);
    assert_eq!(segment.to_string().parse::<SegmentId>(), Ok(segment));
    assert!(
        segment.to_string().parse::<LookupId>().is_ok(),
        "same shape, distinct type"
    );
    assert!(format!("{segment}0").parse::<SegmentId>().is_err());
    assert!("g".repeat(64).parse::<SegmentId>().is_err());
}

#[test]
fn credential_ids_are_sixteen_base32_characters() {
    let id = CredentialId::random();
    assert_eq!(id.to_string().len(), 16);
    assert_eq!(id.to_string().parse::<CredentialId>(), Ok(id));
    assert!(
        id.to_string()
            .to_uppercase()
            .parse::<CredentialId>()
            .is_err()
    );
}

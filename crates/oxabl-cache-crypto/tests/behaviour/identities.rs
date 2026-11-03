use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use age::secrecy::{ExposeSecret, SecretString};
use oxabl_cache_crypto::identity_env::IDENTITY_VARIABLE;
use oxabl_cache_crypto::identity_file::{
    self, WORK_FACTOR, protect, protect_with_work_factor, unprotect, unprotect_with_max_work_factor,
};
use oxabl_cache_crypto::identity_string::parse_descriptor;
use oxabl_cache_crypto::{Error, Identity, MemberEntry};
use oxabl_cache_format::{MemberDescriptor, Role};

fn passphrase(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("identities-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(name)
}

#[test]
fn identities_are_distinct_and_round_trip_through_their_string() {
    let a = Identity::generate().expect("identity");
    let b = Identity::generate().expect("identity");
    assert_ne!(a.signing_public(), b.signing_public());
    assert_ne!(a.recipient(), b.recipient());

    let secret = a.to_secret_string().expect("string");
    assert!(secret.expose_secret().starts_with("OXABL-CACHE-IDENTITY-1"));
    let again = Identity::parse(secret.expose_secret()).expect("parse");
    assert_eq!(again.signing_public(), a.signing_public());
    assert_eq!(again.recipient(), a.recipient());
    assert_eq!(
        again.to_secret_string().expect("string").expose_secret(),
        secret.expose_secret()
    );

    let lower = secret.expose_secret().to_ascii_lowercase();
    assert_eq!(
        Identity::parse(&lower)
            .expect("lower case")
            .signing_public(),
        a.signing_public()
    );
    assert_eq!(
        Identity::parse(&format!("  {}\n", secret.expose_secret()))
            .expect("trimmed")
            .recipient(),
        a.recipient()
    );
}

#[test]
fn malformed_identity_strings_are_refused() {
    let id = Identity::generate().expect("identity");
    let good = id
        .to_secret_string()
        .expect("string")
        .expose_secret()
        .to_owned();
    let letter = good[25..]
        .find(|c: char| c.is_ascii_alphabetic())
        .expect("a letter")
        + 25;
    let mut mixed = good.clone();
    mixed.replace_range(letter..=letter, &good[letter..=letter].to_ascii_lowercase());
    let mut flipped = good.clone().into_bytes();
    flipped[40] = if flipped[40] == b'Q' { b'P' } else { b'Q' };
    let descriptor = id.descriptor_string().expect("descriptor");
    for (what, bad) in [
        ("mixed case", mixed),
        ("bad checksum", String::from_utf8(flipped).expect("ascii")),
        ("truncated", good[..good.len() - 10].to_owned()),
        ("empty", String::new()),
        ("wrong prefix", descriptor),
        ("garbage", "not an identity".to_owned()),
    ] {
        assert!(
            matches!(
                Identity::parse(&bad),
                Err(Error::Identity(_)) | Err(Error::Format(_))
            ),
            "{what}"
        );
    }
}

#[test]
fn debug_output_never_shows_secrets() {
    let id = Identity::generate().expect("identity");
    let secret = id.to_secret_string().expect("string");
    let shown = format!("{id:?}");
    assert!(!shown.contains(secret.expose_secret()));
    assert!(!shown.contains(&secret.expose_secret()[22..40]));
    assert!(shown.contains("Identity"));
}

#[test]
fn the_descriptor_carries_the_public_halves() {
    let id = Identity::generate().expect("identity");
    let text = id.descriptor_string().expect("descriptor");
    let descriptor = MemberDescriptor::decode(&text).expect("decode");
    assert_eq!(descriptor.ed25519_public, id.signing_public());
    assert_eq!(
        parse_descriptor(&text.to_ascii_uppercase()).expect("upper"),
        descriptor
    );
    let member = MemberEntry::from_descriptor("me", Role::Developer, &descriptor).expect("member");
    assert_eq!(member.age_recipients, vec![id.recipient()]);
    assert_eq!(member.sign_keys, vec![id.signing_public()]);
}

#[test]
fn a_protected_file_unlocks_only_with_its_passphrase() {
    let id = Identity::generate().expect("identity");
    let bytes = protect_with_work_factor(&id, &passphrase("correct horse"), 10).expect("protect");
    assert!(!bytes.windows(10).any(|w| w == b"OXABL-CACH"));
    let opened = unprotect(&bytes, &passphrase("correct horse")).expect("unlock");
    assert_eq!(opened.signing_public(), id.signing_public());
    assert!(matches!(
        unprotect(&bytes, &passphrase("wrong")),
        Err(Error::Identity(_))
    ));
    let mut bad = bytes.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(unprotect(&bad, &passphrase("correct horse")).is_err());
    assert!(unprotect(&bytes[..bytes.len() / 2], &passphrase("correct horse")).is_err());
    assert!(unprotect(b"", &passphrase("x")).is_err());
}

#[test]
fn new_files_use_the_documented_work_factor_and_unlock_ceiling() {
    assert_eq!(WORK_FACTOR, 18);
    let id = Identity::generate().expect("identity");
    let bytes = protect(&id, &passphrase("pass")).expect("protect");
    let header = String::from_utf8_lossy(
        &bytes[..bytes
            .iter()
            .position(|&b| b == b'\n')
            .map_or(0, |_| 120.min(bytes.len()))],
    )
    .into_owned();
    let stanza = header
        .lines()
        .find(|l| l.starts_with("-> scrypt"))
        .expect("scrypt stanza");
    assert!(stanza.ends_with(" 18"), "{stanza}");
    assert!(unprotect(&bytes, &passphrase("pass")).is_ok());
}

#[test]
fn a_file_that_demands_more_work_than_the_ceiling_is_refused_without_doing_it() {
    let id = Identity::generate().expect("identity");
    let bytes = protect_with_work_factor(&id, &passphrase("pass"), 12).expect("protect");
    let started = std::time::Instant::now();
    assert!(unprotect_with_max_work_factor(&bytes, &passphrase("pass"), 11).is_err());
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
    assert!(unprotect_with_max_work_factor(&bytes, &passphrase("pass"), 12).is_ok());
    assert!(protect_with_work_factor(&id, &passphrase("pass"), 0).is_err());
    assert!(protect_with_work_factor(&id, &passphrase("pass"), 31).is_err());
}

#[test]
fn saved_files_are_private_and_never_overwritten() {
    let id = Identity::generate().expect("identity");
    let path = scratch("saved");
    let _ = std::fs::remove_file(&path);
    identity_file::save(&path, &id, &passphrase("pass")).expect("save");
    assert_eq!(
        std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        identity_file::save(&path, &id, &passphrase("other")),
        Err(Error::Io(std::io::ErrorKind::AlreadyExists))
    );
    let loaded = identity_file::load(&path, &passphrase("pass")).expect("load");
    assert_eq!(loaded.signing_public(), id.signing_public());
}

#[test]
fn only_regular_files_of_sane_size_load() {
    let dir = scratch("dir-as-file");
    std::fs::create_dir_all(&dir).expect("dir");
    assert!(identity_file::load(&dir, &passphrase("x")).is_err());
    let big = scratch("big");
    std::fs::write(&big, vec![0u8; 70_000]).expect("write");
    assert!(identity_file::load(&big, &passphrase("x")).is_err());
    assert!(identity_file::load(&scratch("missing"), &passphrase("x")).is_err());
}

#[test]
fn the_identity_loads_from_the_environment_variable() {
    if let Ok(expected) = std::env::var("EXPECTED_SIGNING_PUBLIC") {
        let id =
            oxabl_cache_crypto::identity_env::from_env().expect("identity from the environment");
        let hex: String = id
            .signing_public()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, expected);
        return;
    }
    let id = Identity::generate().expect("identity");
    let hex: String = id
        .signing_public()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let run = |value: Option<&str>| {
        let mut cmd = Command::new(std::env::current_exe().expect("exe"));
        cmd.args([
            "--exact",
            "identities::the_identity_loads_from_the_environment_variable",
            "--nocapture",
        ])
        .env("EXPECTED_SIGNING_PUBLIC", &hex)
        .env_remove(IDENTITY_VARIABLE);
        if let Some(v) = value {
            cmd.env(IDENTITY_VARIABLE, v);
        }
        cmd.output().expect("child").status.success()
    };
    assert!(run(Some(
        id.to_secret_string().expect("string").expose_secret()
    )));
    assert!(!run(None), "an unset variable is not an identity");
    assert!(
        !run(Some("OXABL-CACHE-IDENTITY-1NOTVALID")),
        "garbage is not an identity"
    );
}

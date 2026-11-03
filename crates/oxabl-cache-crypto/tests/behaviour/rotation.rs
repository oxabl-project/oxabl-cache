use std::io::Read;
use std::iter;

use oxabl_cache_crypto::{
    Error, Identity, SealOptions, SegmentExpectation, VerifiedRoster, open_segment, seal_segment,
};
use oxabl_cache_format::{Limits, Roster};

use crate::support::data::{Rng, rcode_like};
use crate::support::world::{World, member};

fn signed(w: &World, draft: &oxabl_cache_crypto::RosterDraft) -> VerifiedRoster {
    VerifiedRoster::verify(&draft.sign(&[&w.owner]).expect("sign"), &w.pins, 0).expect("verify")
}

#[test]
fn adding_a_member_keeps_the_epoch_and_the_root_key() {
    let w = World::new();
    let newcomer = Identity::generate().expect("identity");
    let draft = w
        .roster
        .draft_add_member(
            &w.root,
            member("newcomer", oxabl_cache_format::Role::Developer, &newcomer),
        )
        .expect("draft");
    let next = signed(&w, &draft);
    assert_eq!((next.version(), next.epoch()), (2, 1));
    let key = [1u8; 32];
    let expected = w.root.lookup_key().lookup_id(&key);
    for who in [&newcomer, &w.dev, &w.ci, &w.owner] {
        assert_eq!(
            next.unwrap_root_key(who)
                .expect("unwrap")
                .lookup_key()
                .lookup_id(&key),
            expected
        );
    }
}

#[test]
fn a_new_member_cannot_unwrap_the_earlier_roster() {
    let w = World::new();
    let newcomer = Identity::generate().expect("identity");
    assert!(matches!(
        w.roster.unwrap_root_key(&newcomer),
        Err(Error::IdentityNotInRoster)
    ));
}

#[test]
fn removing_a_member_starts_a_new_epoch_they_cannot_read() {
    let w = World::new();
    let draft = w.roster.draft_remove_member("dev").expect("draft");
    let next = signed(&w, &draft);
    assert_eq!((next.version(), next.epoch()), (2, 2));
    assert!(next.members().iter().all(|m| m.label != "dev"));

    let key = [1u8; 32];
    let new_root = next
        .unwrap_root_key(&w.ci)
        .expect("remaining member unwraps");
    assert_ne!(
        new_root.lookup_key().lookup_id(&key),
        w.root.lookup_key().lookup_id(&key),
        "lookup ids change with the epoch"
    );
    assert!(matches!(
        next.unwrap_root_key(&w.dev),
        Err(Error::IdentityNotInRoster)
    ));
}

#[test]
fn a_removed_member_with_the_old_root_key_cannot_open_the_new_epoch() {
    let w = World::new();
    let old_root = w
        .roster
        .unwrap_root_key(&w.dev)
        .expect("the member held the old key");
    let next = signed(&w, &w.roster.draft_remove_member("dev").expect("draft"));
    let new_root = next.unwrap_root_key(&w.ci).expect("unwrap");

    let plain = rcode_like(&mut Rng::new(50), 40_000);
    let mut sealed = Vec::new();
    let info = seal_segment(
        &new_root.segment_key(),
        &plain[..],
        &SealOptions::default(),
        &Limits::default(),
        &mut sealed,
    )
    .expect("seal");
    let opened = open_segment(
        &old_root.segment_key(),
        &sealed,
        &SegmentExpectation {
            id: info.id,
            sealed_len: info.sealed_len,
            plain_len: info.plain_len,
            dictionary: None,
        },
        &Limits::default(),
        Vec::new(),
    );
    assert_eq!(opened, Err(Error::KeyCommitment));
}

#[test]
fn the_new_keyring_is_not_encrypted_to_a_removed_member() {
    let w = World::new();
    let next = signed(&w, &w.roster.draft_remove_member("dev").expect("draft"));
    let wire = Roster::decode(next.as_bytes()).expect("decode");
    let age_identity = age_identity_of(&w.dev);

    let decryptor = age::Decryptor::new(wire.keyring).expect("age header");
    assert!(
        decryptor
            .decrypt(iter::once(&age_identity as &dyn age::Identity))
            .is_err()
    );

    let decryptor = age::Decryptor::new(wire.keyring).expect("age header");
    let mut reader = decryptor
        .decrypt(iter::once(&age_identity_of(&w.ci) as &dyn age::Identity))
        .expect("a remaining member decrypts");
    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext).expect("read");
    assert_eq!(plaintext.len(), 52);
}

fn age_identity_of(identity: &Identity) -> age::x25519::Identity {
    use age::secrecy::ExposeSecret;
    let lower = identity
        .to_secret_string()
        .expect("string")
        .expose_secret()
        .to_ascii_lowercase();
    let payload =
        oxabl_cache_format::bech32::decode(&lower, oxabl_cache_format::bech32::IDENTITY_HRP)
            .expect("bech32");
    let secret =
        oxabl_cache_format::bech32::encode("age-secret-key-", &payload[1..33]).expect("encode");
    secret.to_ascii_uppercase().parse().expect("age identity")
}

#[test]
fn rotation_keeps_the_members_and_replaces_the_key() {
    let w = World::new();
    let next = signed(&w, &w.roster.draft_rotate().expect("draft"));
    assert_eq!((next.version(), next.epoch()), (2, 2));
    assert_eq!(next.members(), w.roster.members());
    let key = [1u8; 32];
    assert_ne!(
        next.unwrap_root_key(&w.dev)
            .expect("unwrap")
            .lookup_key()
            .lookup_id(&key),
        w.root.lookup_key().lookup_id(&key)
    );
}

#[test]
fn removing_an_unknown_member_is_refused() {
    let w = World::new();
    assert!(w.roster.draft_remove_member("nobody").is_err());
}

#[test]
fn once_the_new_roster_is_seen_the_old_one_is_a_rollback() {
    let w = World::new();
    let next = signed(&w, &w.roster.draft_remove_member("dev").expect("draft"));
    assert!(VerifiedRoster::verify(&w.roster_bytes, &w.pins, next.version()).is_err());
    let pins = oxabl_cache_crypto::RosterPins {
        min_version: next.version(),
        ..w.pins.clone()
    };
    assert!(VerifiedRoster::verify(&w.roster_bytes, &pins, 0).is_err());
    assert!(VerifiedRoster::verify(next.as_bytes(), &pins, next.version()).is_ok());
}

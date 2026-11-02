use oxabl_cache_crypto::{Error, Identity, MemberEntry, RosterDraft, RosterPins, VerifiedRoster};
use oxabl_cache_format::{CacheId, Role};

use crate::support::world::{World, member};

fn verify(w: &World, bytes: &[u8]) -> Result<VerifiedRoster, Error> {
    VerifiedRoster::verify(bytes, &w.pins, 0)
}

#[test]
fn every_member_unwraps_the_same_root_key() {
    let w = World::new();
    let key = [42u8; 32];
    let expected = w.root.lookup_key().lookup_id(&key);
    for who in [&w.owner, &w.ci, &w.dev, &w.dev2, &w.reader] {
        let root = w.roster.unwrap_root_key(who).expect("unwrap");
        assert_eq!(root.lookup_key().lookup_id(&key), expected);
    }
}

#[test]
fn an_identity_that_is_not_a_member_cannot_unwrap() {
    let w = World::new();
    let stranger = Identity::generate().expect("identity");
    assert!(matches!(
        w.roster.unwrap_root_key(&stranger),
        Err(Error::IdentityNotInRoster)
    ));
}

#[test]
fn a_roster_for_another_cache_is_refused() {
    let w = World::new();
    let pins = RosterPins {
        cache_id: CacheId([8; 16]),
        ..w.pins.clone()
    };
    assert!(matches!(
        VerifiedRoster::verify(&w.roster_bytes, &pins, 0),
        Err(Error::Roster(_))
    ));
}

#[test]
fn a_version_below_the_pinned_minimum_or_the_high_water_mark_is_refused() {
    let w = World::new();
    let pins = RosterPins {
        min_version: 2,
        ..w.pins.clone()
    };
    assert!(matches!(
        VerifiedRoster::verify(&w.roster_bytes, &pins, 0),
        Err(Error::Roster(_))
    ));
    assert!(matches!(
        VerifiedRoster::verify(&w.roster_bytes, &w.pins, 2),
        Err(Error::Roster(_))
    ));
    assert!(VerifiedRoster::verify(&w.roster_bytes, &w.pins, 1).is_ok());
}

#[test]
fn a_roster_signed_by_a_non_owner_is_refused() {
    let w = World::new();
    let members = w.roster.members().to_vec();
    let draft = RosterDraft::genesis(w.cache_id, members).expect("draft");
    let forged = draft.sign(&[&w.dev]).expect("sign");
    assert!(matches!(verify(&w, &forged), Err(Error::Roster(_))));
}

#[test]
fn a_pinned_signature_is_enough_and_an_unpinned_one_adds_nothing() {
    let w = World::new();
    let second = Identity::generate().expect("identity");
    let draft = RosterDraft::genesis(w.cache_id, w.roster.members().to_vec()).expect("draft");
    let both = draft.sign(&[&second, &w.owner]).expect("sign");
    let accepted = verify(&w, &both).expect("verify");
    assert_eq!(accepted.signers(), [w.owner.signing_public()]);

    let only_unpinned = draft.sign(&[&second]).expect("sign");
    assert!(verify(&w, &only_unpinned).is_err());
}

#[test]
fn a_planned_owner_key_change_works_with_either_pin() {
    let w = World::new();
    let new_owner = Identity::generate().expect("identity");
    let mut members = w.roster.members().to_vec();
    members.push(member("owner2", Role::Owner, &new_owner));
    let draft = RosterDraft::genesis(w.cache_id, members).expect("draft");
    let bytes = draft.sign(&[&w.owner, &new_owner]).expect("sign");
    for pinned in [&w.owner, &new_owner] {
        let pins = RosterPins {
            owner_keys: vec![pinned.signing_public()],
            ..w.pins.clone()
        };
        assert!(VerifiedRoster::verify(&bytes, &pins, 0).is_ok());
    }
}

#[test]
fn every_single_byte_flip_of_a_roster_is_refused() {
    let w = World::new();
    for at in 0..w.roster_bytes.len() {
        let mut bad = w.roster_bytes.clone();
        bad[at] ^= 0x01;
        assert!(verify(&w, &bad).is_err(), "byte {at} of {}", bad.len());
    }
}

#[test]
fn truncated_and_extended_rosters_are_refused() {
    let w = World::new();
    for cut in [1usize, 10, 96, 97, 200] {
        assert!(
            verify(&w, &w.roster_bytes[..w.roster_bytes.len() - cut]).is_err(),
            "cut {cut}"
        );
    }
    let mut longer = w.roster_bytes.clone();
    longer.push(0);
    assert!(verify(&w, &longer).is_err());
    assert!(verify(&w, &[]).is_err());
}

#[test]
fn signatures_are_one_to_four() {
    let w = World::new();
    let draft = RosterDraft::genesis(w.cache_id, w.roster.members().to_vec()).expect("draft");
    assert!(draft.sign(&[]).is_err());
    let owners: Vec<Identity> = (0..5)
        .map(|_| Identity::generate().expect("identity"))
        .collect();
    let refs: Vec<&Identity> = owners.iter().collect();
    assert!(draft.sign(&refs).is_err());
    assert!(draft.sign(&refs[..4]).is_ok());
    assert!(
        draft.sign(&[&owners[0], &owners[0]]).is_err(),
        "one key cannot sign twice"
    );
}

#[test]
fn malformed_member_lists_are_refused() {
    let w = World::new();
    let base = w.roster.members().to_vec();
    let genesis = |members: Vec<MemberEntry>| RosterDraft::genesis(w.cache_id, members).map(|_| ());

    assert!(genesis(vec![]).is_err(), "no members");

    let mut dup_label = base.clone();
    dup_label[1].label = dup_label[0].label.clone();
    assert!(genesis(dup_label).is_err(), "duplicate label");

    let mut dup_key = base.clone();
    dup_key[1].sign_keys = dup_key[0].sign_keys.clone();
    assert!(genesis(dup_key).is_err(), "duplicate signing key");

    let mut dup_recipient = base.clone();
    dup_recipient[1].age_recipients = dup_recipient[0].age_recipients.clone();
    assert!(genesis(dup_recipient).is_err(), "duplicate recipient");

    let mut no_keys = base.clone();
    no_keys[0].sign_keys.clear();
    assert!(genesis(no_keys).is_err(), "member without signing keys");

    let mut bad_recipient = base.clone();
    bad_recipient[0].age_recipients = vec!["age1notarecipient".into()];
    assert!(genesis(bad_recipient).is_err(), "malformed recipient");
}

#[test]
fn the_roster_lists_labels_roles_and_keys_but_never_the_root_key() {
    let w = World::new();
    assert_eq!(w.roster.members().len(), 5);
    assert_eq!(w.roster.version(), 1);
    assert_eq!(w.roster.epoch(), 1);
    assert_eq!(w.roster.cache_id(), w.cache_id);
    let ci = w
        .roster
        .members()
        .iter()
        .find(|m| m.label == "ci")
        .expect("ci");
    assert_eq!(ci.role, Role::Ci);
    assert_eq!(ci.sign_keys, vec![w.ci.signing_public()]);
    let root_bytes = {
        let lookup = w.root.lookup_key().lookup_id(&[0; 32]);
        lookup.0
    };
    assert!(!w.roster_bytes.windows(32).any(|b| b == root_bytes));
}

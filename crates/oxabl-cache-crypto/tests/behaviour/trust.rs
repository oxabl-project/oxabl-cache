use oxabl_cache_crypto::{Error, Identity, open_record, seal_record};
use oxabl_cache_format::{Limits, LookupId, Namespace, SegmentId};

use crate::support::world::{World, fields};

fn lookup() -> LookupId {
    LookupId([8; 32])
}

fn accept(
    world: &World,
    namespace: Namespace,
    signer: &Identity,
    reader: &Identity,
    roster_version: u64,
) -> Result<(), Error> {
    let ctx = world.context(namespace, lookup());
    let sealed = seal_record(
        &world.root.record_key(),
        &ctx,
        &fields(SegmentId([1; 32]), 70_000, 1_000, roster_version),
        signer,
        &Limits::default(),
    )
    .expect("seal");
    open_record(&world.root.record_key(), &ctx, &sealed, &Limits::default())
        .expect("the record itself is valid")
        .accept(&world.roster, &reader.signing_public())
        .map(|_| ())
}

#[test]
fn trusted_records_are_accepted_from_ci_members_only() {
    let w = World::new();
    assert_eq!(accept(&w, Namespace::Trusted, &w.ci, &w.dev, 1), Ok(()));
    for (who, signer) in [
        ("developer", &w.dev),
        ("other developer", &w.dev2),
        ("reader", &w.reader),
        ("owner", &w.owner),
    ] {
        assert_eq!(
            accept(&w, Namespace::Trusted, signer, &w.dev, 1),
            Err(Error::SignerNotAccepted),
            "{who}"
        );
    }
    let stranger = Identity::generate().expect("identity");
    assert_eq!(
        accept(&w, Namespace::Trusted, &stranger, &w.dev, 1),
        Err(Error::SignerNotAccepted)
    );
}

#[test]
fn personal_records_are_accepted_from_the_reader_alone() {
    let w = World::new();
    assert_eq!(accept(&w, Namespace::Personal, &w.dev, &w.dev, 1), Ok(()));
    assert_eq!(
        accept(&w, Namespace::Personal, &w.dev2, &w.dev, 1),
        Err(Error::SignerNotAccepted),
        "another developer's personal entry"
    );
    assert_eq!(
        accept(&w, Namespace::Personal, &w.ci, &w.dev, 1),
        Err(Error::SignerNotAccepted),
        "a ci key in a personal namespace"
    );
}

#[test]
fn a_reader_who_is_not_in_the_roster_accepts_nothing() {
    let w = World::new();
    let stranger = Identity::generate().expect("identity");
    assert_eq!(
        accept(&w, Namespace::Personal, &stranger, &stranger, 1),
        Err(Error::SignerNotAccepted)
    );
}

#[test]
fn a_record_from_the_future_of_the_roster_is_refused() {
    let w = World::new();
    assert_eq!(w.roster.version(), 1);
    assert_eq!(
        accept(&w, Namespace::Trusted, &w.ci, &w.dev, 2),
        Err(Error::RosterVersionAhead)
    );
    assert_eq!(
        accept(&w, Namespace::Personal, &w.dev, &w.dev, 2),
        Err(Error::RosterVersionAhead)
    );
}

#[test]
fn a_signer_the_current_roster_no_longer_lists_is_refused_whatever_the_epoch() {
    let w = World::new();
    let ctx = w.context(Namespace::Trusted, lookup());
    let sealed = seal_record(
        &w.root.record_key(),
        &ctx,
        &fields(SegmentId([1; 32]), 70_000, 1_000, 1),
        &w.ci,
        &Limits::default(),
    )
    .expect("seal");
    let opened =
        open_record(&w.root.record_key(), &ctx, &sealed, &Limits::default()).expect("open");
    assert!(
        opened
            .clone()
            .accept(&w.roster, &w.dev.signing_public())
            .is_ok()
    );

    let draft = w.roster.draft_remove_member("ci").expect("draft");
    let next = oxabl_cache_crypto::VerifiedRoster::verify(
        &draft.sign(&[&w.owner]).expect("sign"),
        &w.pins,
        0,
    )
    .expect("verify");
    assert_eq!(
        opened.accept(&next, &w.dev.signing_public()).map(|_| ()),
        Err(Error::SignerNotAccepted)
    );
}

#[test]
fn an_accepted_record_reports_its_provenance() {
    let w = World::new();
    let ctx = w.context(Namespace::Trusted, lookup());
    let sealed = seal_record(
        &w.root.record_key(),
        &ctx,
        &fields(SegmentId([1; 32]), 70_000, 1_000, 1),
        &w.ci,
        &Limits::default(),
    )
    .expect("seal");
    let accepted = open_record(&w.root.record_key(), &ctx, &sealed, &Limits::default())
        .expect("open")
        .accept(&w.roster, &w.dev.signing_public())
        .expect("accept");
    let p = accepted.provenance();
    assert_eq!(p.tier, Namespace::Trusted);
    assert_eq!(p.signer, w.ci.signing_public());
    assert_eq!((p.epoch, p.roster_version), (1, 1));
}

//! A fixed cache for the crypto fuzz targets: one owner, one member, a signed roster.

use std::sync::OnceLock;

use oxabl_cache_crypto::{
    Identity, MemberEntry, RecordContext, RootKey, RosterDraft, RosterPins, VerifiedRoster,
};
use oxabl_cache_format::{CacheId, LookupId, Namespace, Role};

pub struct Fixture {
    pub cache_id: CacheId,
    pub owner: Identity,
    pub member: Identity,
    pub pins: RosterPins,
    pub roster_bytes: Vec<u8>,
    pub roster: VerifiedRoster,
    pub root: RootKey,
}

impl Fixture {
    pub fn context(&self) -> RecordContext {
        RecordContext {
            cache_id: self.cache_id,
            epoch: self.roster.epoch(),
            namespace: Namespace::Trusted,
            lookup_id: LookupId([5; 32]),
        }
    }
}

fn member(label: &str, role: Role, identity: &Identity) -> MemberEntry {
    MemberEntry::from_descriptor(label, role, &identity.descriptor().expect("descriptor"))
        .expect("member")
}

pub fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let cache_id = CacheId([7; 16]);
        let owner = Identity::generate().expect("identity");
        let member_identity = Identity::generate().expect("identity");
        let draft = RosterDraft::genesis(
            cache_id,
            vec![
                member("owner", Role::Owner, &owner),
                member("ci", Role::Ci, &member_identity),
            ],
        )
        .expect("draft");
        let roster_bytes = draft.sign(&[&owner]).expect("sign");
        let pins = RosterPins {
            cache_id,
            owner_keys: vec![owner.signing_public()],
            min_version: 1,
        };
        let roster = VerifiedRoster::verify(&roster_bytes, &pins, 0).expect("verify");
        let root = roster.unwrap_root_key(&owner).expect("root key");
        Fixture {
            cache_id,
            owner,
            member: member_identity,
            pins,
            roster_bytes,
            roster,
            root,
        }
    })
}

use oxabl_cache_crypto::{
    EntryFields, FileEntry, Identity, MemberEntry, RecordContext, RootKey, RosterDraft, RosterPins,
    VerifiedRoster,
};
use oxabl_cache_format::{CacheId, LookupId, Namespace, Role, SegmentId};

/// A cache with an owner, a CI producer, two developers and a reader, at roster version 1.
pub struct World {
    pub cache_id: CacheId,
    pub owner: Identity,
    pub ci: Identity,
    pub dev: Identity,
    pub dev2: Identity,
    pub reader: Identity,
    pub pins: RosterPins,
    pub roster_bytes: Vec<u8>,
    pub roster: VerifiedRoster,
    pub root: RootKey,
}

pub fn member(label: &str, role: Role, identity: &Identity) -> MemberEntry {
    MemberEntry::from_descriptor(label, role, &identity.descriptor().expect("descriptor"))
        .expect("member")
}

impl World {
    pub fn new() -> Self {
        let cache_id = CacheId([7; 16]);
        let owner = Identity::generate().expect("identity");
        let ci = Identity::generate().expect("identity");
        let dev = Identity::generate().expect("identity");
        let dev2 = Identity::generate().expect("identity");
        let reader = Identity::generate().expect("identity");
        let draft = RosterDraft::genesis(
            cache_id,
            vec![
                member("owner", Role::Owner, &owner),
                member("ci", Role::Ci, &ci),
                member("dev", Role::Developer, &dev),
                member("dev2", Role::Developer, &dev2),
                member("reader", Role::Reader, &reader),
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
        Self {
            cache_id,
            owner,
            ci,
            dev,
            dev2,
            reader,
            pins,
            roster_bytes,
            roster,
            root,
        }
    }

    pub fn context(&self, namespace: Namespace, lookup_id: LookupId) -> RecordContext {
        RecordContext {
            cache_id: self.cache_id,
            epoch: self.roster.epoch(),
            namespace,
            lookup_id,
        }
    }
}

pub fn fields(
    segment_id: SegmentId,
    segment_len: u32,
    plain_len: u64,
    roster_version: u64,
) -> EntryFields {
    EntryFields {
        segment_id,
        segment_len,
        dict_id: None,
        plain_len,
        created_at: 1_793_000_000,
        roster_version,
        files: vec![FileEntry {
            path: "r/ar/customer.r".into(),
            size: plain_len,
        }],
    }
}

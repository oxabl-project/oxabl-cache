//! Signing keys held by the test, never by the server.

use ed25519_dalek::{Signer, SigningKey};
use oxabl_cache_core::domain::roster::{
    DictionaryPointer, MemberRole, Roster, RosterMember, Signature,
};
use oxabl_cache_core::ids::SegmentId;

pub struct Keys {
    owner: SigningKey,
    pub ci: SigningKey,
    pub stranger: SigningKey,
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

impl Keys {
    pub fn new() -> Self {
        Self {
            owner: key(1),
            ci: key(2),
            stranger: key(3),
        }
    }

    pub fn owner_public(&self) -> [u8; 32] {
        self.owner.verifying_key().to_bytes()
    }

    fn sign(key: &SigningKey, message: &[u8]) -> Signature {
        Signature {
            key: key.verifying_key().to_bytes(),
            signature: key.sign(message).to_bytes(),
        }
    }

    pub fn roster(&self, version: u64, epoch: u32, signer: &SigningKey) -> Roster {
        let signed_message = format!("roster v{version} e{epoch}").into_bytes();
        Roster {
            version,
            epoch,
            bytes: signed_message.clone(),
            members: vec![
                RosterMember {
                    role: MemberRole::Owner,
                    sign_keys: vec![self.owner.verifying_key().to_bytes()],
                },
                RosterMember {
                    role: MemberRole::Ci,
                    sign_keys: vec![self.ci.verifying_key().to_bytes()],
                },
            ],
            signatures: vec![Self::sign(signer, &signed_message)],
            signed_message,
        }
    }

    pub fn owner_signed_roster(&self, version: u64, epoch: u32) -> Roster {
        self.roster(version, epoch, &self.owner)
    }

    pub fn pointer(
        &self,
        version: u64,
        epoch: u32,
        segment_id: SegmentId,
        signer: &SigningKey,
    ) -> DictionaryPointer {
        let signed_message = format!("dictionary v{version} {segment_id}").into_bytes();
        DictionaryPointer {
            version,
            epoch,
            segment_id,
            bytes: signed_message.clone(),
            signature: Self::sign(signer, &signed_message),
            signed_message,
        }
    }

    pub fn ci_pointer(&self, version: u64, epoch: u32, segment_id: SegmentId) -> DictionaryPointer {
        self.pointer(version, epoch, segment_id, &self.ci)
    }
}

//! `OXRS` roster: members, keyring and owner signatures.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::{read_header, write_header};
use crate::ids::{CacheId, PublicKey, Signature};
use crate::limits::{ROSTER_BYTES, ROSTER_MEMBERS, ROSTER_SIGNATURES};
use crate::writer::{Writer, len32};

const MAGIC: [u8; 4] = *b"OXRS";
const MEMBER_MIN: usize = 2 + 1 + 1 + 1;
const SIGNATURE_LEN: usize = 32 + 64;

/// What a member may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Ci,
    Developer,
    Reader,
}

impl Role {
    fn to_u8(self) -> u8 {
        match self {
            Self::Owner => 0,
            Self::Ci => 1,
            Self::Developer => 2,
            Self::Reader => 3,
        }
    }

    fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::Owner),
            1 => Ok(Self::Ci),
            2 => Ok(Self::Developer),
            3 => Ok(Self::Reader),
            _ => Err(FormatError::Invalid("role")),
        }
    }
}

/// One roster member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member<'a> {
    pub label: &'a str,
    pub role: Role,
    pub sign_keys: Vec<PublicKey>,
    pub age_recipients: Vec<&'a str>,
}

/// One owner signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterSignature {
    pub owner_key: PublicKey,
    pub signature: Signature,
}

/// A roster. `keyring` is an age file, opaque here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster<'a> {
    pub cache_id: CacheId,
    pub version: u64,
    pub epoch: u32,
    pub members: Vec<Member<'a>>,
    pub keyring: &'a [u8],
    pub signatures: Vec<RosterSignature>,
}

impl<'a> Roster<'a> {
    fn write_signed(&self) -> Result<Writer> {
        if self.members.len() > ROSTER_MEMBERS {
            return Err(FormatError::LimitExceeded("roster members"));
        }
        let mut w = Writer::default();
        write_header(&mut w, MAGIC, 0);
        w.bytes(&self.cache_id.0);
        w.u64(self.version);
        w.u32(self.epoch);
        w.u16(self.members.len() as u16);
        for m in &self.members {
            w.str16(m.label)?;
            w.u8(m.role.to_u8());
            w.u8(u8::try_from(m.sign_keys.len())
                .map_err(|_| FormatError::LimitExceeded("sign keys"))?);
            for k in &m.sign_keys {
                w.bytes(k);
            }
            w.u8(u8::try_from(m.age_recipients.len())
                .map_err(|_| FormatError::LimitExceeded("age recipients"))?);
            for r in &m.age_recipients {
                w.str16(r)?;
            }
        }
        w.u32(len32(self.keyring.len(), "keyring")?);
        w.bytes(self.keyring);
        Ok(w)
    }

    /// Every byte before `sig_count`: what each owner signature covers, after the caller's
    /// domain-separated prefix.
    pub fn signed_bytes(&self) -> Result<Vec<u8>> {
        let w = self.write_signed()?;
        if w.len() > ROSTER_BYTES {
            return Err(FormatError::LimitExceeded("roster size"));
        }
        Ok(w.into_vec())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        if !(1..=ROSTER_SIGNATURES).contains(&self.signatures.len()) {
            return Err(FormatError::Invalid("signature count"));
        }
        let mut w = self.write_signed()?;
        w.u8(self.signatures.len() as u8);
        for s in &self.signatures {
            w.bytes(&s.owner_key);
            w.bytes(&s.signature);
        }
        if w.len() > ROSTER_BYTES {
            return Err(FormatError::LimitExceeded("roster size"));
        }
        Ok(w.into_vec())
    }

    pub fn decode(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > ROSTER_BYTES {
            return Err(FormatError::LimitExceeded("roster size"));
        }
        let mut c = Cursor::new(bytes);
        read_header(&mut c, MAGIC, 0)?;
        let cache_id = CacheId(c.array()?);
        let version = c.u64()?;
        let epoch = c.u32()?;
        let member_count = c.count16(ROSTER_MEMBERS, "roster members", MEMBER_MIN)?;
        let mut members = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let label = c.str16()?;
            let role = Role::from_u8(c.u8()?)?;
            let n = c.u8()? as usize;
            c.check_count(n as u64, usize::MAX, "sign keys", 32)?;
            let mut sign_keys = Vec::with_capacity(n);
            for _ in 0..n {
                sign_keys.push(c.array()?);
            }
            let m = c.u8()? as usize;
            c.check_count(m as u64, usize::MAX, "age recipients", 2)?;
            let mut age_recipients = Vec::with_capacity(m);
            for _ in 0..m {
                age_recipients.push(c.str16()?);
            }
            members.push(Member {
                label,
                role,
                sign_keys,
                age_recipients,
            });
        }
        let keyring_len = c.u32()? as usize;
        let keyring = c.take(keyring_len)?;
        let sig_count = c.u8()? as usize;
        if !(1..=ROSTER_SIGNATURES).contains(&sig_count) {
            return Err(FormatError::Invalid("signature count"));
        }
        c.check_count(
            sig_count as u64,
            ROSTER_SIGNATURES,
            "roster signatures",
            SIGNATURE_LEN,
        )?;
        let mut signatures = Vec::with_capacity(sig_count);
        for _ in 0..sig_count {
            signatures.push(RosterSignature {
                owner_key: c.array()?,
                signature: c.array()?,
            });
        }
        c.finish()?;
        Ok(Self {
            cache_id,
            version,
            epoch,
            members,
            keyring,
            signatures,
        })
    }
}

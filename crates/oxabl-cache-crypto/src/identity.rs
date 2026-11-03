//! Member identities: one X25519 key to receive the root key and one Ed25519 key to sign.

use age::secrecy::SecretString;
use ed25519_dalek::{Signer, SigningKey};
use oxabl_cache_format::{MemberDescriptor, PublicKey, Signature};
use zeroize::{ZeroizeOnDrop, Zeroizing};

use crate::error::{Error, Result};
use crate::identity_string::{
    IdentitySecrets, age_recipient_bytes, age_secret_string, decode_identity, encode_identity,
};
use crate::random;

/// A member's secret identity. Zeroizes on drop; `Debug` shows only the public keys.
pub struct Identity {
    x25519_secret: Zeroizing<[u8; 32]>,
    x25519: age::x25519::Identity,
    signing: SigningKey,
}

impl Identity {
    /// A new identity from the operating system's random generator.
    pub fn generate() -> Result<Self> {
        Self::from_secrets(IdentitySecrets {
            x25519: Zeroizing::new(random::array()?),
            ed25519_seed: Zeroizing::new(random::array()?),
        })
    }

    pub(crate) fn from_secrets(secrets: IdentitySecrets) -> Result<Self> {
        let age_secret = age_secret_string(&secrets.x25519)?;
        let x25519 = age_secret
            .parse::<age::x25519::Identity>()
            .map_err(|_| Error::Identity("invalid X25519 secret"))?;
        Ok(Self {
            x25519,
            signing: SigningKey::from_bytes(&secrets.ed25519_seed),
            x25519_secret: secrets.x25519,
        })
    }

    /// Parses an `OXABL-CACHE-IDENTITY-1…` string, in upper or lower case.
    pub fn parse(s: &str) -> Result<Self> {
        Self::from_secrets(decode_identity(s)?)
    }

    /// The identity string. Anyone who sees it holds the identity.
    pub fn to_secret_string(&self) -> Result<SecretString> {
        let encoded = encode_identity(&IdentitySecrets {
            x25519: self.x25519_secret.clone(),
            ed25519_seed: Zeroizing::new(self.signing.to_bytes()),
        })?;
        Ok(SecretString::from(encoded.to_string()))
    }

    /// The Ed25519 public key that signs this member's entries.
    pub fn signing_public(&self) -> PublicKey {
        self.signing.verifying_key().to_bytes()
    }

    /// The `age1…` recipient that receives the root key.
    pub fn recipient(&self) -> String {
        self.x25519.to_public().to_string()
    }

    /// The public halves, for `member add`.
    pub fn descriptor(&self) -> Result<MemberDescriptor> {
        Ok(MemberDescriptor {
            ed25519_public: self.signing_public(),
            x25519_public: age_recipient_bytes(&self.recipient())?,
        })
    }

    /// The Bech32 string of [`Identity::descriptor`].
    pub fn descriptor_string(&self) -> Result<String> {
        Ok(self.descriptor()?.encode()?)
    }

    pub(crate) fn sign(&self, message: &[u8]) -> Signature {
        self.signing.sign(message).to_bytes()
    }

    pub(crate) fn age_identity(&self) -> &age::x25519::Identity {
        &self.x25519
    }
}

/// Every secret field zeroizes on drop: the X25519 secret, the age identity and the signing key.
impl ZeroizeOnDrop for Identity {}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("signing_public", &self.signing_public())
            .field("recipient", &self.recipient())
            .finish_non_exhaustive()
    }
}

//! JSON Web Keys and signature verification for RS256, ES256 and EdDSA.

use aws_lc_rs::rsa::PublicKeyComponents;
use aws_lc_rs::signature::{
    ECDSA_P256_SHA256_FIXED, ED25519, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey,
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;

const MAX_KEYS: usize = 64;

/// The signature algorithms the verifier accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Algorithm {
    Rs256,
    Es256,
    EdDsa,
}

impl Algorithm {
    /// Names only the three allowed algorithms; `none`, HMAC and the rest are `None`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "RS256" => Some(Self::Rs256),
            "ES256" => Some(Self::Es256),
            "EdDSA" => Some(Self::EdDsa),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum Material {
    Rsa { n: Vec<u8>, e: Vec<u8> },
    Ec { point: Vec<u8> },
    Ed { key: Vec<u8> },
}

/// A public signing key of an issuer.
#[derive(Debug, Clone)]
pub(crate) struct Jwk {
    kid: Option<String>,
    alg: Option<String>,
    material: Material,
}

fn b64(value: &Value, field: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(value.get(field)?.as_str()?).ok()
}

fn padded(bytes: Vec<u8>, width: usize) -> Option<Vec<u8>> {
    (bytes.len() <= width).then(|| {
        let mut out = vec![0u8; width - bytes.len()];
        out.extend(bytes);
        out
    })
}

impl Jwk {
    fn parse(value: &Value) -> Option<Self> {
        if value.get("use").is_some_and(|usage| usage != "sig") {
            return None;
        }
        let material = match value.get("kty")?.as_str()? {
            "RSA" => {
                let n = b64(value, "n")?;
                let bits = n.iter().skip_while(|b| **b == 0).count() * 8;
                if !(2048..=8192).contains(&bits) {
                    return None;
                }
                Material::Rsa {
                    n,
                    e: b64(value, "e")?,
                }
            }
            "EC" if value.get("crv")? == "P-256" => {
                let x = padded(b64(value, "x")?, 32)?;
                let y = padded(b64(value, "y")?, 32)?;
                Material::Ec {
                    point: [&[4u8][..], &x, &y].concat(),
                }
            }
            "OKP" if value.get("crv")? == "Ed25519" => {
                let key = b64(value, "x")?;
                (key.len() == 32).then_some(Material::Ed { key })?
            }
            _ => return None,
        };
        Some(Self {
            kid: value.get("kid").and_then(Value::as_str).map(str::to_owned),
            alg: value.get("alg").and_then(Value::as_str).map(str::to_owned),
            material,
        })
    }

    pub fn kid(&self) -> Option<&str> {
        self.kid.as_deref()
    }

    /// Whether the key can verify `algorithm`.
    pub fn fits(&self, algorithm: Algorithm) -> bool {
        let family = matches!(
            (&self.material, algorithm),
            (Material::Rsa { .. }, Algorithm::Rs256)
                | (Material::Ec { .. }, Algorithm::Es256)
                | (Material::Ed { .. }, Algorithm::EdDsa)
        );
        let named = match (&self.alg, algorithm) {
            (None, _) => true,
            (Some(name), algorithm) => Algorithm::parse(name) == Some(algorithm),
        };
        family && named
    }

    /// Verifies `signature` over `message`.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        match &self.material {
            Material::Rsa { n, e } => PublicKeyComponents {
                n: n.as_slice(),
                e: e.as_slice(),
            }
            .verify(&RSA_PKCS1_2048_8192_SHA256, message, signature)
            .is_ok(),
            Material::Ec { point } => UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
                .verify(message, signature)
                .is_ok(),
            Material::Ed { key } => UnparsedPublicKey::new(&ED25519, key)
                .verify(message, signature)
                .is_ok(),
        }
    }
}

/// The usable keys of a JWKS document; unusable entries are dropped.
pub(crate) fn parse_jwks(bytes: &[u8]) -> Option<Vec<Jwk>> {
    let document: Value = serde_json::from_slice(bytes).ok()?;
    let keys = document.get("keys")?.as_array()?;
    Some(keys.iter().take(MAX_KEYS).filter_map(Jwk::parse).collect())
}

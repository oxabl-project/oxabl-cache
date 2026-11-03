//! Signing keys of the fake identity provider.

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::rsa::{KeyPair as RsaKeyPair, KeySize, PublicKeyComponents};
use aws_lc_rs::signature::{
    ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair, RSA_PKCS1_SHA256,
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

/// The algorithms the server accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alg {
    Rs256,
    Es256,
    EdDsa,
}

enum Pair {
    Rsa(RsaKeyPair),
    Ec(EcdsaKeyPair),
    Ed(Ed25519KeyPair),
}

/// A key pair with its `kid`.
pub struct SigningKey {
    pub kid: String,
    pub alg: Alg,
    pair: Pair,
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

impl SigningKey {
    pub fn generate(alg: Alg, kid: &str) -> Self {
        let pair = match alg {
            Alg::Rs256 => Pair::Rsa(RsaKeyPair::generate(KeySize::Rsa2048).expect("rsa key")),
            Alg::Es256 => {
                Pair::Ec(EcdsaKeyPair::generate(&ECDSA_P256_SHA256_FIXED_SIGNING).expect("ec key"))
            }
            Alg::EdDsa => Pair::Ed(Ed25519KeyPair::generate().expect("ed key")),
        };
        Self {
            kid: kid.to_owned(),
            alg,
            pair,
        }
    }

    pub fn alg_name(&self) -> &'static str {
        match self.alg {
            Alg::Rs256 => "RS256",
            Alg::Es256 => "ES256",
            Alg::EdDsa => "EdDSA",
        }
    }

    pub fn jwk(&self) -> Value {
        match &self.pair {
            Pair::Rsa(pair) => {
                let parts = PublicKeyComponents::<Vec<u8>>::from(pair.public_key());
                json!({"kty": "RSA", "use": "sig", "alg": "RS256", "kid": self.kid,
                       "n": b64(&parts.n), "e": b64(&parts.e)})
            }
            Pair::Ec(pair) => {
                let point = pair.public_key().as_ref();
                json!({"kty": "EC", "crv": "P-256", "use": "sig", "alg": "ES256", "kid": self.kid,
                       "x": b64(&point[1..33]), "y": b64(&point[33..65])})
            }
            Pair::Ed(pair) => json!({"kty": "OKP", "crv": "Ed25519", "use": "sig", "alg": "EdDSA",
                       "kid": self.kid, "x": b64(pair.public_key().as_ref())}),
        }
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        match &self.pair {
            Pair::Rsa(pair) => {
                let mut signature = vec![0u8; pair.public_modulus_len()];
                pair.sign(
                    &RSA_PKCS1_SHA256,
                    &SystemRandom::new(),
                    message,
                    &mut signature,
                )
                .expect("rsa signature");
                signature
            }
            Pair::Ec(pair) => pair
                .sign(&SystemRandom::new(), message)
                .expect("ec signature")
                .as_ref()
                .to_vec(),
            Pair::Ed(pair) => pair.sign(message).as_ref().to_vec(),
        }
    }

    /// The public key bytes, the secret of an algorithm-confusion forgery.
    pub fn public_bytes(&self) -> Vec<u8> {
        match &self.pair {
            Pair::Rsa(pair) => pair.public_key().as_ref().to_vec(),
            Pair::Ec(pair) => pair.public_key().as_ref().to_vec(),
            Pair::Ed(pair) => pair.public_key().as_ref().to_vec(),
        }
    }
}

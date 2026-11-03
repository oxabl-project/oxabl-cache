//! A throwaway certificate authority and the leaf certificates it issues.

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

pub struct TestCa {
    issuer: CertifiedIssuer<'static, KeyPair>,
}

impl TestCa {
    pub fn new() -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, "oxabl test ca");
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let issuer =
            CertifiedIssuer::self_signed(params, KeyPair::generate().expect("ca key")).expect("ca");
        Self { issuer }
    }

    pub fn pem(&self) -> String {
        self.issuer.pem()
    }

    /// A server certificate valid for the given DNS names.
    pub fn issue(&self, names: &[&str]) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        let mut params =
            CertificateParams::new(names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>())
                .expect("leaf params");
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        let key = KeyPair::generate().expect("leaf key");
        let cert = params.signed_by(&key, &*self.issuer).expect("leaf");
        (
            vec![cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
    }
}

impl Default for TestCa {
    fn default() -> Self {
        Self::new()
    }
}

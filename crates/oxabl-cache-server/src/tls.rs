//! TLS 1.3 from certificate files, with rustls.

use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

/// An acceptor that speaks TLS 1.3 only, offering `h2` and `http/1.1`.
pub(crate) fn acceptor(cert: &Path, key: &Path) -> Result<TlsAcceptor, String> {
    let certs = CertificateDer::pem_file_iter(cert)
        .map_err(|error| format!("cannot read certificate {}: {error}", cert.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("bad certificate {}: {error}", cert.display()))?;
    if certs.is_empty() {
        return Err(format!("{} holds no certificate", cert.display()));
    }
    let key = PrivateKeyDer::from_pem_file(key)
        .map_err(|error| format!("cannot read private key {}: {error}", key.display()))?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| format!("TLS setup: {error}"))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|error| format!("certificate and key do not fit: {error}"))?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(config)))
}

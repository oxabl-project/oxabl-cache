//! A TLS client over rustls that trusts one certificate and offers chosen versions.

use super::http::{Response, head_text, parse};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, StreamOwned, SupportedProtocolVersion,
};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

fn config(cert_pem: &str, versions: &[&'static SupportedProtocolVersion]) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    let cert = CertificateDer::from_pem_slice(cert_pem.as_bytes()).expect("certificate");
    roots.add(cert).expect("trust the certificate");
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    Arc::new(
        ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(versions)
            .expect("versions")
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// One request over TLS; the error is the handshake or transport failure.
pub fn request(
    addr: &str,
    cert_pem: &str,
    versions: &[&'static SupportedProtocolVersion],
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> Result<Response, String> {
    let tcp = TcpStream::connect(addr).map_err(|error| error.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    let name = ServerName::try_from("localhost").expect("name");
    let connection =
        ClientConnection::new(config(cert_pem, versions), name).map_err(|e| e.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let mut all = headers.to_vec();
    all.push(("Content-Length", body.len().to_string()));
    stream
        .write_all(head_text(method, path, &all).as_bytes())
        .and_then(|()| stream.write_all(body))
        .map_err(|error| error.to_string())?;
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    if raw.is_empty() {
        return Err("no response".to_owned());
    }
    Ok(parse(&raw))
}

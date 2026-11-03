//! TLS for the database connection: rustls with the public web roots, or a private CA file.

use oxabl_cache_core::{Error, Result};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_postgres::Socket;
use tokio_postgres::tls::{ChannelBinding, MakeTlsConnect, TlsConnect, TlsStream};
use tokio_rustls::TlsConnector;

/// Builds connections' TLS layer; every certificate is verified against the roots.
#[derive(Clone)]
pub(crate) struct Tls {
    config: Arc<ClientConfig>,
}

impl Tls {
    /// Trusts the public web roots, or only the certificates in `ca_file`.
    pub(crate) fn new(ca_file: Option<&Path>) -> Result<Self> {
        let mut roots = RootCertStore::empty();
        match ca_file {
            None => {
                roots.add_parsable_certificates(
                    webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().cloned(),
                );
            }
            Some(path) => {
                let certs = CertificateDer::pem_file_iter(path)
                    .map_err(|error| ca_error(path, &error))?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|error| ca_error(path, &error))?;
                let (added, _) = roots.add_parsable_certificates(certs);
                if added == 0 {
                    return Err(Error::unavailable(format!(
                        "metadata ca_file {} holds no certificate",
                        path.display()
                    )));
                }
            }
        }
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| Error::unavailable(format!("metadata tls: {error}")))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            config: Arc::new(config),
        })
    }
}

fn ca_error(path: &Path, error: &dyn std::fmt::Display) -> Error {
    Error::unavailable(format!(
        "cannot read metadata ca_file {}: {error}",
        path.display()
    ))
}

impl MakeTlsConnect<Socket> for Tls {
    type Stream = Stream;
    type TlsConnect = Connect;
    type Error = io::Error;

    fn make_tls_connect(&mut self, domain: &str) -> io::Result<Connect> {
        let name = ServerName::try_from(domain.to_owned())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        Ok(Connect {
            connector: TlsConnector::from(Arc::clone(&self.config)),
            name,
        })
    }
}

pub(crate) struct Connect {
    connector: TlsConnector,
    name: ServerName<'static>,
}

impl TlsConnect<Socket> for Connect {
    type Stream = Stream;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = io::Result<Stream>> + Send>>;

    fn connect(self, socket: Socket) -> Self::Future {
        Box::pin(async move {
            let stream = self.connector.connect(self.name, socket).await?;
            Ok(Stream(stream))
        })
    }
}

pub(crate) struct Stream(tokio_rustls::client::TlsStream<Socket>);

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl TlsStream for Stream {
    fn channel_binding(&self) -> ChannelBinding {
        ChannelBinding::none()
    }
}

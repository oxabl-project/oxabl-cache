//! The guarded HTTPS GET behind every discovery and key-set fetch.

use crate::address::permitted;
use crate::settings::OidcSettings;
use crate::target::Target;
use http_body_util::{BodyExt, Empty, Limited};
use hyper::body::Bytes;
use hyper::header::{ACCEPT, CONNECTION, HOST, LOCATION};
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use rustls::pki_types::{ServerName, pem::PemObject};
use rustls::{ClientConfig, RootCertStore};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use url::Host;

/// Largest accepted response body.
pub(crate) const MAX_BODY: usize = 256 * 1024;
const TIME_BUDGET: Duration = Duration::from_secs(5);
const MAX_REDIRECTS: usize = 3;

/// A fetch that gave nothing; the cause is deliberately not carried to callers.
#[derive(Debug)]
pub(crate) struct FetchError;

/// Fetches issuer documents: the URL and every redirect are vetted, the connection goes
/// to the address that was checked, and size and time are capped.
pub(crate) struct Fetcher {
    settings: OidcSettings,
    tls: TlsConnector,
}

impl Fetcher {
    pub fn new(settings: OidcSettings) -> Result<Self, String> {
        let mut roots = RootCertStore::empty();
        let native = rustls_native_certs::load_native_certs();
        roots.add_parsable_certificates(native.certs);
        if let Some(pem) = &settings.extra_ca_pem {
            for cert in rustls::pki_types::CertificateDer::pem_slice_iter(pem) {
                let cert = cert.map_err(|error| format!("extra CA file: {error}"))?;
                roots
                    .add(cert)
                    .map_err(|error| format!("extra CA file: {error}"))?;
            }
        }
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| format!("TLS setup: {error}"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            settings,
            tls: TlsConnector::from(Arc::new(config)),
        })
    }

    pub fn settings(&self) -> &OidcSettings {
        &self.settings
    }

    /// GETs `url` and returns the body of a 200 answer.
    pub async fn get(&self, url: &str) -> Result<Bytes, FetchError> {
        tokio::time::timeout(TIME_BUDGET, self.follow(url))
            .await
            .map_err(|_| FetchError)?
    }

    async fn follow(&self, url: &str) -> Result<Bytes, FetchError> {
        let mut current = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            let target = Target::parse(&current, &self.settings.allowed_private_ranges)
                .map_err(|_| FetchError)?;
            let (status, location, body) = self.once(&target).await?;
            match status {
                200 => return Ok(body),
                301 | 302 | 303 | 307 | 308 => {
                    let location = location.ok_or(FetchError)?;
                    current = target.join(&location).ok_or(FetchError)?;
                }
                _ => return Err(FetchError),
            }
        }
        Err(FetchError)
    }

    async fn addresses(&self, target: &Target) -> Result<Vec<IpAddr>, FetchError> {
        match &target.host {
            Host::Ipv4(ip) => Ok(vec![IpAddr::V4(*ip)]),
            Host::Ipv6(ip) => Ok(vec![IpAddr::V6(*ip)]),
            Host::Domain(name) => {
                let found = self
                    .settings
                    .resolver
                    .lookup(name)
                    .await
                    .map_err(|_| FetchError)?;
                let ranges = &self.settings.allowed_private_ranges;
                if found.iter().all(|ip| permitted(*ip, ranges)) {
                    Ok(found)
                } else {
                    Err(FetchError)
                }
            }
        }
    }

    async fn once(&self, target: &Target) -> Result<(u16, Option<String>, Bytes), FetchError> {
        let mut stream = None;
        for ip in self.addresses(target).await? {
            if let Ok(connected) = TcpStream::connect(SocketAddr::new(ip, 443)).await {
                stream = Some(connected);
                break;
            }
        }
        let stream = stream.ok_or(FetchError)?;
        let server_name = match &target.host {
            Host::Domain(name) => ServerName::try_from(name.clone()).map_err(|_| FetchError)?,
            Host::Ipv4(ip) => ServerName::IpAddress(IpAddr::V4(*ip).into()),
            Host::Ipv6(ip) => ServerName::IpAddress(IpAddr::V6(*ip).into()),
        };
        let tls = self
            .tls
            .connect(server_name, stream)
            .await
            .map_err(|_| FetchError)?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
            .await
            .map_err(|_| FetchError)?;
        let driver = tokio::spawn(async move {
            let _ = connection.await;
        });
        let request = Request::builder()
            .method(Method::GET)
            .uri(&target.path_and_query)
            .header(HOST, target.authority())
            .header(ACCEPT, "application/json")
            .header(CONNECTION, "close")
            .body(Empty::<Bytes>::new())
            .map_err(|_| FetchError)?;
        let outcome = async {
            let response = sender.send_request(request).await.map_err(|_| FetchError)?;
            let status = response.status().as_u16();
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let body = Limited::new(response.into_body(), MAX_BODY)
                .collect()
                .await
                .map_err(|_| FetchError)?
                .to_bytes();
            Ok((status, location, body))
        }
        .await;
        driver.abort();
        outcome
    }
}

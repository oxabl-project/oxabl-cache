//! A fake OIDC provider: discovery and key-set documents over TLS, a request log,
//! key rotation and redirects, plus a decoy that only counts connections.

use super::ca::TestCa;
use super::device::{Device, DeviceGrant};
use super::keys::SigningKey;
use rustls::ServerConfig;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

const MAX_BODY: usize = 16 * 1024;

/// One request the provider answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub sni: Option<String>,
    pub host: String,
    pub path: String,
}

#[derive(Default)]
struct IssuerState {
    keys: Vec<Arc<SigningKey>>,
    discovery_redirect: Option<String>,
    jwks_padding: usize,
    stall_jwks: bool,
    device: Option<DeviceGrant>,
    claimed_issuer: Option<String>,
}

#[derive(Default)]
struct State {
    issuers: Mutex<HashMap<String, IssuerState>>,
    seen: Mutex<Vec<Seen>>,
    connections: AtomicUsize,
}

/// The provider's listener. Dropping it stops serving.
pub struct Idp {
    state: Arc<State>,
    task: JoinHandle<()>,
}

fn respond(status: &str, extra: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

enum Answer {
    Bytes(Vec<u8>),
    Stall,
}

fn route(state: &State, host: &str, method: &str, path: &str, body: &str) -> Answer {
    let mut issuers = state.issuers.lock().expect("issuers");
    for (prefix, issuer) in issuers.iter_mut() {
        if path == format!("{prefix}/.well-known/openid-configuration") {
            if let Some(location) = &issuer.discovery_redirect {
                return Answer::Bytes(respond(
                    "302 Found",
                    &format!("Location: {location}\r\n"),
                    "",
                ));
            }
            let mut document = json!({
                "issuer": issuer
                    .claimed_issuer
                    .clone()
                    .unwrap_or_else(|| format!("https://{host}{prefix}")),
                "jwks_uri": format!("https://{host}{prefix}/jwks"),
            });
            if issuer.device.is_some() {
                document["device_authorization_endpoint"] =
                    json!(format!("https://{host}{prefix}/device"));
                document["token_endpoint"] = json!(format!("https://{host}{prefix}/token"));
            }
            return Answer::Bytes(respond("200 OK", "", &document.to_string()));
        }
        if let (Some(grant), "POST") = (issuer.device.as_mut(), method) {
            let base = format!("https://{host}{prefix}");
            let answer = if path == format!("{prefix}/device") {
                Some(grant.authorize(&base, body))
            } else if path == format!("{prefix}/token") {
                Some(grant.token(&base, body))
            } else {
                None
            };
            if let Some((status, document)) = answer {
                return Answer::Bytes(respond(status, "", &document.to_string()));
            }
        }
        if path == format!("{prefix}/jwks") {
            if issuer.stall_jwks {
                return Answer::Stall;
            }
            let keys: Vec<_> = issuer.keys.iter().map(|key| key.jwk()).collect();
            let body = json!({ "keys": keys, "padding": "x".repeat(issuer.jwks_padding) });
            return Answer::Bytes(respond("200 OK", "", &body.to_string()));
        }
    }
    Answer::Bytes(respond("404 Not Found", "", "{}"))
}

async fn serve(state: Arc<State>, acceptor: TlsAcceptor, listener: TcpListener) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        state.connections.fetch_add(1, Ordering::SeqCst);
        let (state, acceptor) = (Arc::clone(&state), acceptor.clone());
        tokio::spawn(async move {
            let Ok(mut tls) = acceptor.accept(stream).await else {
                return;
            };
            let sni = tls.get_ref().1.server_name().map(str::to_owned);
            let mut head = Vec::new();
            let mut buffer = [0u8; 1024];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 8192 {
                match tls.read(&mut buffer).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => head.extend_from_slice(&buffer[..n]),
                }
            }
            let end = head
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map_or(head.len(), |at| at + 4);
            let text = String::from_utf8_lossy(&head[..end]).into_owned();
            let mut request_line = text.lines().next().unwrap_or("").split(' ');
            let method = request_line.next().unwrap_or("").to_owned();
            let path = request_line.next().unwrap_or("").to_owned();
            let host = text
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Host: ")
                        .or_else(|| line.strip_prefix("host: "))
                })
                .unwrap_or("")
                .to_owned();
            let length: usize = text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().ok())?
                })
                .unwrap_or(0)
                .min(MAX_BODY);
            let mut body = head[end..].to_vec();
            while body.len() < length {
                match tls.read(&mut buffer).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => body.extend_from_slice(&buffer[..n]),
                }
            }
            let body = String::from_utf8_lossy(&body[..length.min(body.len())]).into_owned();
            state.seen.lock().expect("seen").push(Seen {
                sni,
                host: host.clone(),
                path: path.clone(),
            });
            match route(&state, &host, &method, &path, &body) {
                Answer::Bytes(bytes) => {
                    let _ = tls.write_all(&bytes).await;
                    let _ = tls.shutdown().await;
                }
                Answer::Stall => tokio::time::sleep(std::time::Duration::from_secs(60)).await,
            }
        });
    }
}

impl Idp {
    /// Serves TLS on `addr`, with a certificate for `names` from `ca`.
    pub async fn start(addr: SocketAddr, ca: &TestCa, names: &[&str]) -> Self {
        let (certs, key) = ca.issue(names);
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("protocols")
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .expect("certificate");
        let listener = TcpListener::bind(addr).await.expect("bind the provider");
        let state = Arc::new(State::default());
        let task = tokio::spawn(serve(
            Arc::clone(&state),
            TlsAcceptor::from(Arc::new(config)),
            listener,
        ));
        Self { state, task }
    }

    /// Publishes `keys` as the key set of the issuer at path `prefix`.
    pub fn publish(&self, prefix: &str, keys: Vec<Arc<SigningKey>>) {
        let mut issuers = self.state.issuers.lock().expect("issuers");
        issuers.entry(prefix.to_owned()).or_default().keys = keys;
    }

    /// Answers discovery of the issuer at `prefix` with a redirect.
    pub fn redirect_discovery(&self, prefix: &str, location: &str) {
        let mut issuers = self.state.issuers.lock().expect("issuers");
        issuers
            .entry(prefix.to_owned())
            .or_default()
            .discovery_redirect = Some(location.to_owned());
    }

    /// Pads the key-set document of the issuer at `prefix` by `bytes`.
    pub fn pad_jwks(&self, prefix: &str, bytes: usize) {
        let mut issuers = self.state.issuers.lock().expect("issuers");
        issuers.entry(prefix.to_owned()).or_default().jwks_padding = bytes;
    }

    /// Accepts requests for the key set of the issuer at `prefix` and never answers.
    pub fn stall_jwks(&self, prefix: &str) {
        let mut issuers = self.state.issuers.lock().expect("issuers");
        issuers.entry(prefix.to_owned()).or_default().stall_jwks = true;
    }

    /// Makes discovery of the issuer at `prefix` name `issuer` instead of itself.
    pub fn claim_issuer(&self, prefix: &str, issuer: &str) {
        let mut issuers = self.state.issuers.lock().expect("issuers");
        issuers.entry(prefix.to_owned()).or_default().claimed_issuer = Some(issuer.to_owned());
    }

    /// Makes the issuer at `prefix` serve the device authorization grant.
    pub fn device(&self, prefix: &str, device: Device) {
        let mut issuers = self.state.issuers.lock().expect("issuers");
        issuers.entry(prefix.to_owned()).or_default().device = Some(DeviceGrant::new(device));
    }

    /// The bodies of the device authorization requests the issuer at `prefix` received.
    pub fn device_requests(&self, prefix: &str) -> Vec<String> {
        let issuers = self.state.issuers.lock().expect("issuers");
        issuers
            .get(prefix)
            .and_then(|issuer| issuer.device.as_ref())
            .map(|grant| grant.requests.clone())
            .unwrap_or_default()
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.state.seen.lock().expect("seen").clone()
    }

    pub fn seen_paths(&self, prefix: &str) -> usize {
        self.seen()
            .iter()
            .filter(|seen| seen.path.starts_with(prefix))
            .count()
    }

    pub fn connections(&self) -> usize {
        self.state.connections.load(Ordering::SeqCst)
    }
}

impl Drop for Idp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A listener that only counts the connections it receives.
pub struct Decoy {
    count: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Decoy {
    pub async fn start(addr: SocketAddr) -> Self {
        let listener = TcpListener::bind(addr).await.expect("bind the decoy");
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        let task = tokio::spawn(async move {
            while let Ok((_stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        Self { count, task }
    }

    pub fn connections(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

impl Drop for Decoy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

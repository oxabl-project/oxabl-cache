//! A fake OIDC provider: discovery and key-set documents over TLS, a request log,
//! key rotation and redirects, plus a decoy that only counts connections.

use super::ca::TestCa;
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

fn route(state: &State, host: &str, path: &str) -> Answer {
    let issuers = state.issuers.lock().expect("issuers");
    for (prefix, issuer) in issuers.iter() {
        if path == format!("{prefix}/.well-known/openid-configuration") {
            if let Some(location) = &issuer.discovery_redirect {
                return Answer::Bytes(respond(
                    "302 Found",
                    &format!("Location: {location}\r\n"),
                    "",
                ));
            }
            let body = json!({
                "issuer": format!("https://{host}{prefix}"),
                "jwks_uri": format!("https://{host}{prefix}/jwks"),
            });
            return Answer::Bytes(respond("200 OK", "", &body.to_string()));
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
            let text = String::from_utf8_lossy(&head).into_owned();
            let path = text
                .lines()
                .next()
                .and_then(|line| line.split(' ').nth(1))
                .unwrap_or("")
                .to_owned();
            let host = text
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Host: ")
                        .or_else(|| line.strip_prefix("host: "))
                })
                .unwrap_or("")
                .to_owned();
            state.seen.lock().expect("seen").push(Seen {
                sni,
                host: host.clone(),
                path: path.clone(),
            });
            match route(&state, &host, &path) {
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

//! A CI system's token endpoint, GitHub Actions style, that records what it is asked for.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use http_body_util::Full;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::task::JoinHandle;

/// A JWT-shaped token with the given audience; the signature is not one, the server's to check.
pub fn jwt(audience: &str) -> String {
    let part = |value: serde_json::Value| URL_SAFE_NO_PAD.encode(value.to_string());
    format!(
        "{}.{}.{}",
        part(serde_json::json!({"alg": "RS256", "kid": "k1"})),
        part(
            serde_json::json!({"iss": "https://token.example", "aud": audience, "sub": "repo:acme/app"})
        ),
        URL_SAFE_NO_PAD.encode([7u8; 32]),
    )
}

#[derive(Clone, Debug)]
pub struct Asked {
    pub audience: Option<String>,
    pub authorization: Option<String>,
    pub query: String,
}

pub struct Issuer {
    pub address: SocketAddr,
    asked: Arc<Mutex<Vec<Asked>>>,
    task: JoinHandle<()>,
}

impl Issuer {
    /// Serves tokens for the audience asked for, or for `always` when that is set.
    pub async fn start(always: Option<String>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
        let log = asked.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let (log, always) = (log.clone(), always.clone());
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                        let (log, always) = (log.clone(), always.clone());
                        async move {
                            let query = request.uri().query().unwrap_or_default().to_owned();
                            let audience = url::form_urlencoded::parse(query.as_bytes())
                                .find(|(k, _)| k == "audience")
                                .map(|(_, v)| v.into_owned());
                            log.lock().expect("log").push(Asked {
                                audience: audience.clone(),
                                authorization: request
                                    .headers()
                                    .get("authorization")
                                    .and_then(|v| v.to_str().ok())
                                    .map(str::to_owned),
                                query,
                            });
                            let minted = always.or(audience).unwrap_or_default();
                            let body = serde_json::json!({"value": jwt(&minted)}).to_string();
                            Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(body))))
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        Self {
            address,
            asked,
            task,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}/token?api-version=2", self.address)
    }

    pub fn asked(&self) -> Vec<Asked> {
        self.asked.lock().expect("log").clone()
    }
}

impl Drop for Issuer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

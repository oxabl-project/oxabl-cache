//! One accept loop: optional TLS, HTTP/1.1 and HTTP/2, and a graceful drain.

use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::{GracefulShutdown, Watcher};
use hyper_util::service::TowerToHyperService;
use oxabl_cache_http::log::{Level, emit};
use serde_json::Value;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_rustls::TlsAcceptor;

const HEADER_TIMEOUT: Duration = Duration::from_secs(10);

fn builder() -> Builder<TokioExecutor> {
    let mut builder = Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT);
    builder
}

async fn handle(stream: TcpStream, tls: Option<TlsAcceptor>, router: Router, watcher: Watcher) {
    let service = TowerToHyperService::new(router);
    let builder = builder();
    match tls {
        Some(acceptor) => match acceptor.accept(stream).await {
            Ok(secured) => {
                let connection =
                    builder.serve_connection_with_upgrades(TokioIo::new(secured), service);
                let _ = watcher.watch(connection.into_owned()).await;
            }
            Err(error) => emit(
                Level::Debug,
                "tls_handshake_failed",
                vec![("error", Value::from(error.to_string()))],
            ),
        },
        None => {
            let connection = builder.serve_connection_with_upgrades(TokioIo::new(stream), service);
            let _ = watcher.watch(connection.into_owned()).await;
        }
    }
}

/// Accepts connections until `stop` changes, then waits up to `drain` for the
/// requests in flight.
pub(crate) async fn run(
    listener: TcpListener,
    router: Router,
    tls: Option<TlsAcceptor>,
    mut stop: watch::Receiver<bool>,
    drain: Duration,
) {
    let graceful = GracefulShutdown::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };
                let (tls, router, watcher) = (tls.clone(), router.clone(), graceful.watcher());
                tokio::spawn(handle(stream, tls, router, watcher));
            }
            _ = stop.changed() => break,
        }
    }
    drop(listener);
    let _ = tokio::time::timeout(drain, graceful.shutdown()).await;
}

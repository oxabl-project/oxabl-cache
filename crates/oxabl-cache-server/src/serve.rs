//! `serve`: bind the listeners, run the background work, drain on a signal.

use crate::config::{Config, Transport};
use crate::{keys, listener, meta, scheduler, tls, wiring};
use oxabl_cache_http::log::{self, Level, emit};
use oxabl_cache_http::{admin_router, metrics_router, public_router};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

async fn bind(name: &str, addr: std::net::SocketAddr) -> Result<TcpListener, String> {
    TcpListener::bind(addr)
        .await
        .map_err(|error| format!("cannot bind {name} {addr}: {error}"))
}

/// Runs the server until SIGTERM or SIGINT, then drains and flushes.
pub(crate) async fn run(config: Config) -> Result<(), String> {
    log::set_level(config.log_level);
    for path in keys::secret_files(&config) {
        keys::check_permissions(path, |message| {
            emit(
                Level::Warn,
                "config",
                vec![("message", Value::from(message))],
            );
        });
    }
    let acceptor = match &config.transport {
        Transport::Tls { cert, key } => Some(tls::acceptor(cert, key)?),
        Transport::Plaintext => None,
    };
    let mut terminate = signal(SignalKind::terminate()).map_err(|error| error.to_string())?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(|error| error.to_string())?;

    let public = bind("server.listen", config.listen).await?;
    let admin = bind("server.admin_listen", config.admin_listen).await?;
    let metrics = bind("server.metrics_listen", config.metrics_listen).await?;
    let local = |listener: &TcpListener| listener.local_addr().map_err(|error| error.to_string());
    let (public_addr, admin_addr, metrics_addr) =
        (local(&public)?, local(&admin)?, local(&metrics)?);
    let scheme = if acceptor.is_some() { "https" } else { "http" };
    let public_url = config
        .public_url
        .clone()
        .unwrap_or_else(|| format!("{scheme}://{public_addr}"));

    let app = wiring::build(&config, &public_url).await?;
    app.engine
        .heartbeat()
        .await
        .map_err(|error| format!("cannot reach the metadata database: {error}"))?;

    let (stop, stopped) = watch::channel(false);
    let election = meta::Election::new(&config.metadata)?;
    let background = scheduler::spawn(&app, config.gc, election, &stopped).await;
    let drain = config.shutdown_timeout;
    let serving = vec![
        tokio::spawn(listener::run(
            public,
            public_router(Arc::clone(&app)),
            acceptor.clone(),
            stopped.clone(),
            drain,
        )),
        tokio::spawn(listener::run(
            admin,
            admin_router(Arc::clone(&app)),
            acceptor,
            stopped.clone(),
            drain,
        )),
        tokio::spawn(listener::run(
            metrics,
            metrics_router(Arc::clone(&app)),
            None,
            stopped.clone(),
            drain,
        )),
    ];

    let ready = json!({"ready": {
        "listen": public_addr.to_string(),
        "admin_listen": admin_addr.to_string(),
        "metrics_listen": metrics_addr.to_string(),
        "public_url": public_url,
    }});
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{ready}")
        .and_then(|()| stdout.flush())
        .map_err(|error| error.to_string())?;
    drop(stdout);

    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
    emit(Level::Info, "shutdown", Vec::new());
    let _ = stop.send(true);
    for task in serving.into_iter().chain(background) {
        let _ = task.await;
    }
    scheduler::flush_counters(&app).await;
    Ok(())
}

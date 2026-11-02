//! A TCP proxy that can delay, cut or drop traffic in front of a real service.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::sleep;

/// Which way the traffic flows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Client to upstream.
    Request,
    /// Upstream to client.
    Reply,
}

/// What the proxy does to one direction of every connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Pass,
    /// Waits before forwarding each chunk.
    Delay(Duration),
    /// Closes the connection once this many bytes have been forwarded.
    CutAfter(u64),
    /// Silently discards everything after this many bytes and keeps the connection open.
    DropAfter(u64),
}

#[derive(Clone, Copy)]
struct Faults {
    request: Fault,
    reply: Fault,
}

/// A listening proxy. Dropping it closes the listener and every connection.
pub struct FaultProxy {
    addr: SocketAddr,
    faults: Arc<Mutex<Faults>>,
    cut: watch::Sender<u64>,
    accept: JoinHandle<()>,
}

impl FaultProxy {
    /// Listens on a free loopback port and forwards to `upstream`. Must run inside a tokio runtime.
    pub async fn start(upstream: SocketAddr) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let faults = Arc::new(Mutex::new(Faults {
            request: Fault::Pass,
            reply: Fault::Pass,
        }));
        let (cut, cut_rx) = watch::channel(0u64);
        let accept = tokio::spawn({
            let faults = faults.clone();
            async move {
                while let Ok((client, _)) = listener.accept().await {
                    tokio::spawn(connection(client, upstream, faults.clone(), cut_rx.clone()));
                }
            }
        });
        Ok(Self {
            addr,
            faults,
            cut,
            accept,
        })
    }

    /// The address clients connect to.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Sets the fault for one direction; it applies to chunks forwarded from now on, and byte
    /// counts restart per connection.
    pub fn set(&self, direction: Direction, fault: Fault) {
        let mut faults = self.faults.lock().expect("fault state");
        match direction {
            Direction::Request => faults.request = fault,
            Direction::Reply => faults.reply = fault,
        }
    }

    /// Closes every open connection now.
    pub fn cut_connections(&self) {
        self.cut.send_modify(|generation| *generation += 1);
    }
}

impl Drop for FaultProxy {
    fn drop(&mut self) {
        self.accept.abort();
        self.cut_connections();
    }
}

async fn connection(
    client: TcpStream,
    upstream: SocketAddr,
    faults: Arc<Mutex<Faults>>,
    cut: watch::Receiver<u64>,
) {
    let Ok(upstream) = TcpStream::connect(upstream).await else {
        return;
    };
    let (client_read, client_write) = client.into_split();
    let (upstream_read, upstream_write) = upstream.into_split();
    let request = pump(
        client_read,
        upstream_write,
        faults.clone(),
        Direction::Request,
    );
    let reply = pump(upstream_read, client_write, faults, Direction::Reply);
    let mut cut = cut;
    tokio::select! {
        _ = request => {}
        _ = reply => {}
        _ = cut.changed() => {}
    }
}

async fn pump(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    faults: Arc<Mutex<Faults>>,
    direction: Direction,
) {
    let mut forwarded = 0u64;
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let Ok(n @ 1..) = from.read(&mut buf).await else {
            return;
        };
        let fault = {
            let faults = faults.lock().expect("fault state");
            match direction {
                Direction::Request => faults.request,
                Direction::Reply => faults.reply,
            }
        };
        let chunk = &buf[..n];
        let allowed = match fault {
            Fault::Pass => chunk.len(),
            Fault::Delay(delay) => {
                sleep(delay).await;
                chunk.len()
            }
            Fault::CutAfter(limit) | Fault::DropAfter(limit) => {
                limit.saturating_sub(forwarded).min(chunk.len() as u64) as usize
            }
        };
        if allowed > 0 && to.write_all(&chunk[..allowed]).await.is_err() {
            return;
        }
        forwarded += allowed as u64;
        if allowed < chunk.len() && matches!(fault, Fault::CutAfter(_)) {
            return;
        }
    }
}

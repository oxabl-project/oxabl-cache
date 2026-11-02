//! The fault proxy forwards bytes and can delay, cut or drop them.

use std::future::Future;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use oxabl_cache_e2e::fault_proxy::{Direction, Fault, FaultProxy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("timed out")
}

async fn echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut read, mut write) = stream.split();
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
        }
    });
    addr
}

async fn round_trip(addr: SocketAddr, payload: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(payload).await?;
    let mut reply = vec![0u8; payload.len()];
    stream.read_exact(&mut reply).await?;
    Ok(reply)
}

#[tokio::test]
async fn passes_bytes_unchanged() {
    let proxy = FaultProxy::start(echo_server().await).await.unwrap();

    assert_eq!(
        within(round_trip(proxy.addr(), b"hello")).await.unwrap(),
        b"hello"
    );
}

#[tokio::test]
async fn delays_replies() {
    let proxy = FaultProxy::start(echo_server().await).await.unwrap();
    proxy.set(Direction::Reply, Fault::Delay(Duration::from_millis(300)));

    let started = Instant::now();
    assert_eq!(
        within(round_trip(proxy.addr(), b"hello")).await.unwrap(),
        b"hello"
    );

    assert!(started.elapsed() >= Duration::from_millis(300));
}

#[tokio::test]
async fn cuts_the_connection_after_n_reply_bytes() {
    let proxy = FaultProxy::start(echo_server().await).await.unwrap();
    proxy.set(Direction::Reply, Fault::CutAfter(3));

    let mut stream = TcpStream::connect(proxy.addr()).await.unwrap();
    stream.write_all(b"hello").await.unwrap();
    let mut received = Vec::new();
    within(stream.read_to_end(&mut received)).await.unwrap();

    assert_eq!(received, b"hel");
}

#[tokio::test]
async fn drops_reply_bytes_after_n_and_keeps_the_connection_open() {
    let proxy = FaultProxy::start(echo_server().await).await.unwrap();
    proxy.set(Direction::Reply, Fault::DropAfter(3));

    let mut stream = TcpStream::connect(proxy.addr()).await.unwrap();
    stream.write_all(b"hello").await.unwrap();
    let mut first = [0u8; 3];
    stream.read_exact(&mut first).await.unwrap();
    let rest = tokio::time::timeout(Duration::from_millis(300), stream.read(&mut [0u8; 8])).await;

    assert_eq!(&first, b"hel");
    assert!(rest.is_err(), "the connection stayed silent");
}

#[tokio::test]
async fn cut_connections_closes_open_connections() {
    let proxy = FaultProxy::start(echo_server().await).await.unwrap();
    let mut stream = TcpStream::connect(proxy.addr()).await.unwrap();
    stream.write_all(b"ping").await.unwrap();
    stream.read_exact(&mut [0u8; 4]).await.unwrap();

    proxy.cut_connections();

    let mut rest = Vec::new();
    within(stream.read_to_end(&mut rest)).await.unwrap();
    assert!(rest.is_empty());
}

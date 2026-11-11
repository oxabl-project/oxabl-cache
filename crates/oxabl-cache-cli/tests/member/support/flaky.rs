//! A TCP proxy in front of the server that misbehaves once, in a chosen way: it loses the first
//! roster publication, or widens the scopes of the first token request.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

#[derive(Clone, Copy, PartialEq)]
pub enum Fault {
    /// The server stores the roster, the answer never reaches the client.
    LoseAnswer,
    /// The request never reaches the server.
    LoseRequest,
    /// Like `LoseRequest`, and the proxy refuses every later connection.
    LoseRequestAndGoDark,
    /// The first token request asks the server for `tenant:admin` instead of `write:trusted`.
    WidenScopes,
}

pub struct Flaky {
    address: SocketAddr,
}

struct State {
    fault: Fault,
    armed: AtomicBool,
    dark: AtomicBool,
}

fn is_roster_put(chunk: &[u8]) -> bool {
    let line = chunk.split(|b| *b == b'\r').next().unwrap_or(chunk);
    line.starts_with(b"PUT ") && line.windows(7).any(|w| w == b"/roster")
}

/// Rewrites `"write:trusted"` to `"tenant:admin"` and a space, so the request keeps its length.
fn widen(chunk: &mut [u8], armed: &AtomicBool) {
    const FROM: &[u8] = br#""read","write:trusted""#;
    const TO: &[u8] = br#""read","tenant:admin" "#;
    if let Some(at) = chunk.windows(FROM.len()).position(|w| w == FROM)
        && armed.swap(false, Ordering::SeqCst)
    {
        chunk[at..at + FROM.len()].copy_from_slice(TO);
    }
}

fn serve(client: TcpStream, upstream: SocketAddr, state: &State) {
    let Ok(server) = TcpStream::connect(upstream) else {
        return;
    };
    let cut = Arc::new(AtomicBool::new(false));
    let (Ok(mut from_client), Ok(mut to_server), Ok(mut from_server), Ok(mut to_client)) = (
        client.try_clone(),
        server.try_clone(),
        server.try_clone(),
        client.try_clone(),
    ) else {
        return;
    };
    let answers_cut = cut.clone();
    let (both_client, both_server) = (client, server);
    thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        while let Ok(n) = from_server.read(&mut buffer) {
            if n == 0
                || answers_cut.load(Ordering::SeqCst)
                || to_client.write_all(&buffer[..n]).is_err()
            {
                break;
            }
        }
        let _ = to_client.shutdown(Shutdown::Both);
    });
    let mut buffer = [0u8; 16 * 1024];
    while let Ok(n) = from_client.read(&mut buffer) {
        if n == 0 {
            break;
        }
        if state.fault == Fault::WidenScopes {
            widen(&mut buffer[..n], &state.armed);
        } else if is_roster_put(&buffer[..n]) && state.armed.swap(false, Ordering::SeqCst) {
            match state.fault {
                Fault::LoseAnswer => cut.store(true, Ordering::SeqCst),
                Fault::WidenScopes => {}
                Fault::LoseRequest => break,
                Fault::LoseRequestAndGoDark => {
                    state.dark.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
        if to_server.write_all(&buffer[..n]).is_err() {
            break;
        }
    }
    if !cut.load(Ordering::SeqCst) {
        let _ = both_client.shutdown(Shutdown::Both);
        let _ = both_server.shutdown(Shutdown::Both);
    }
}

impl Flaky {
    /// Forwards to `upstream` and applies `fault` to the first roster publication.
    pub fn start(upstream: SocketAddr, fault: Fault) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("proxy port");
        let address = listener.local_addr().expect("proxy address");
        let state = Arc::new(State {
            fault,
            armed: AtomicBool::new(true),
            dark: AtomicBool::new(false),
        });
        thread::spawn(move || {
            for client in listener.incoming().flatten() {
                if state.dark.load(Ordering::SeqCst) {
                    continue;
                }
                let state = state.clone();
                thread::spawn(move || serve(client, upstream, &state));
            }
        });
        Self { address }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

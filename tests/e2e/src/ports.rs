//! Free TCP port allocation.

use std::collections::HashSet;
use std::net::TcpListener;
use std::sync::Mutex;

static HANDED_OUT: Mutex<Option<HashSet<u16>>> = Mutex::new(None);

/// Returns a loopback port that is free now and has not been handed out by this process before.
pub fn free_port() -> u16 {
    loop {
        let port = TcpListener::bind(("127.0.0.1", 0))
            .and_then(|listener| listener.local_addr())
            .expect("bind a loopback port")
            .port();
        let mut handed_out = HANDED_OUT.lock().expect("port registry");
        if handed_out.get_or_insert_with(HashSet::new).insert(port) {
            return port;
        }
    }
}

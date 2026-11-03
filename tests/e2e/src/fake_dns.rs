//! A fake DNS server whose answers the test changes while the server under test runs.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

#[derive(Default)]
struct Zone {
    answers: HashMap<String, Vec<Ipv4Addr>>,
    alternating: HashMap<String, (Ipv4Addr, Ipv4Addr)>,
    queries: AtomicUsize,
}

pub struct FakeDns {
    zone: Arc<Mutex<Zone>>,
    task: JoinHandle<()>,
    pub addr: SocketAddr,
}

fn question_end(packet: &[u8]) -> Option<(String, usize)> {
    let mut at = 12;
    let mut labels = Vec::new();
    loop {
        let length = usize::from(*packet.get(at)?);
        at += 1;
        if length == 0 {
            break;
        }
        labels.push(String::from_utf8_lossy(packet.get(at..at + length)?).to_ascii_lowercase());
        at += length;
    }
    Some((labels.join("."), at + 4))
}

fn answer(zone: &Zone, packet: &[u8]) -> Option<Vec<u8>> {
    let (name, end) = question_end(packet)?;
    let kind = u16::from_be_bytes([packet[end - 4], packet[end - 3]]);
    let query = if kind == 1 {
        zone.queries.fetch_add(1, Ordering::SeqCst)
    } else {
        0
    };
    let addresses: Vec<Ipv4Addr> = if kind != 1 {
        Vec::new()
    } else if let Some((first, second)) = zone.alternating.get(&name) {
        vec![if query % 2 == 0 { *first } else { *second }]
    } else {
        zone.answers.get(&name).cloned().unwrap_or_default()
    };
    let known = zone.answers.contains_key(&name) || zone.alternating.contains_key(&name);
    let mut reply = Vec::new();
    reply.extend_from_slice(&packet[..2]);
    reply.extend_from_slice(&[0x81, if known { 0x80 } else { 0x83 }, 0, 1]);
    reply.extend_from_slice(&(addresses.len() as u16).to_be_bytes());
    reply.extend_from_slice(&[0, 0, 0, 0]);
    reply.extend_from_slice(&packet[12..end]);
    for address in addresses {
        reply.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 0, 0, 4]);
        reply.extend_from_slice(&address.octets());
    }
    Some(reply)
}

impl FakeDns {
    pub async fn start(addr: SocketAddr) -> Self {
        let socket = UdpSocket::bind(addr).await.expect("bind fake dns");
        let addr = socket.local_addr().expect("address");
        let zone = Arc::new(Mutex::new(Zone::default()));
        let shared = Arc::clone(&zone);
        let task = tokio::spawn(async move {
            let mut buffer = [0u8; 512];
            while let Ok((length, peer)) = socket.recv_from(&mut buffer).await {
                let reply = answer(&shared.lock().expect("zone"), &buffer[..length]);
                if let Some(reply) = reply {
                    let _ = socket.send_to(&reply, peer).await;
                }
            }
        });
        Self { zone, task, addr }
    }

    pub fn set(&self, name: &str, address: &str) {
        let ip: IpAddr = address.parse().expect("address");
        let IpAddr::V4(ip) = ip else {
            panic!("v4 only")
        };
        let mut zone = self.zone.lock().expect("zone");
        zone.alternating.remove(name);
        zone.answers.insert(name.to_owned(), vec![ip]);
    }

    /// Answers `first` and `second` in turn, one per query.
    pub fn alternate(&self, name: &str, first: &str, second: &str) {
        let parse = |text: &str| match text.parse::<IpAddr>().expect("address") {
            IpAddr::V4(ip) => ip,
            IpAddr::V6(_) => panic!("v4 only"),
        };
        let mut zone = self.zone.lock().expect("zone");
        zone.answers.remove(name);
        zone.alternating
            .insert(name.to_owned(), (parse(first), parse(second)));
    }
}

impl Drop for FakeDns {
    fn drop(&mut self) {
        self.task.abort();
    }
}

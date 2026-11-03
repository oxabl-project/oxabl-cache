//! Name resolution done by the server itself, on the system resolver or on
//! configured DNS servers.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;

const MAX_ADDRESSES: usize = 16;
const SERVER_TIMEOUT: Duration = Duration::from_secs(2);
const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;

/// How names become addresses.
#[derive(Debug, Clone)]
pub enum Resolver {
    /// The operating system's resolver.
    System,
    /// These DNS servers, tried in order, over UDP.
    Servers(Vec<SocketAddr>),
}

impl Resolver {
    /// The addresses of `name`, at most 16, without duplicates.
    pub async fn lookup(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        let mut found: Vec<IpAddr> = Vec::new();
        match self {
            Self::System => {
                for address in tokio::net::lookup_host((name, 443)).await? {
                    found.push(address.ip());
                }
            }
            Self::Servers(servers) => {
                let mut last = io::Error::other("no DNS server configured");
                for server in servers {
                    match query_both(*server, name).await {
                        Ok(addresses) if !addresses.is_empty() => {
                            found = addresses;
                            break;
                        }
                        Ok(_) => last = io::Error::other("the name has no address"),
                        Err(error) => last = error,
                    }
                }
                if found.is_empty() {
                    return Err(last);
                }
            }
        }
        let mut unique = Vec::new();
        for ip in found {
            if !unique.contains(&ip) && unique.len() < MAX_ADDRESSES {
                unique.push(ip);
            }
        }
        if unique.is_empty() {
            return Err(io::Error::other("the name has no address"));
        }
        Ok(unique)
    }
}

async fn query_both(server: SocketAddr, name: &str) -> io::Result<Vec<IpAddr>> {
    let (v4, v6) = tokio::join!(query(server, name, TYPE_A), query(server, name, TYPE_AAAA));
    match (v4, v6) {
        (Err(a), Err(_)) => Err(a),
        (a, b) => Ok(a.into_iter().chain(b).flatten().collect()),
    }
}

fn encode_query(id: u16, name: &str, kind: u16) -> io::Result<Vec<u8>> {
    let mut packet = Vec::with_capacity(32 + name.len());
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(io::Error::other("malformed name"));
        }
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&kind.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    if packet.len() > 512 {
        return Err(io::Error::other("name too long"));
    }
    Ok(packet)
}

async fn query(server: SocketAddr, name: &str, kind: u16) -> io::Result<Vec<IpAddr>> {
    let mut id = [0u8; 2];
    getrandom::fill(&mut id).map_err(io::Error::other)?;
    let id = u16::from_be_bytes(id);
    let bind: SocketAddr = if server.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(bind).await?;
    socket.connect(server).await?;
    socket.send(&encode_query(id, name, kind)?).await?;
    let mut buffer = [0u8; 1500];
    let received = tokio::time::timeout(SERVER_TIMEOUT, async {
        loop {
            let length = socket.recv(&mut buffer).await?;
            if length >= 12 && u16::from_be_bytes([buffer[0], buffer[1]]) == id {
                return io::Result::Ok(length);
            }
        }
    })
    .await
    .map_err(|_| io::Error::other("DNS server timed out"))??;
    parse_answers(&buffer[..received])
}

fn skip_name(packet: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let length = *packet.get(at)?;
        match length {
            0 => return Some(at + 1),
            l if l & 0xc0 == 0xc0 => return Some(at + 2),
            l if l & 0xc0 == 0 => at += 1 + usize::from(l),
            _ => return None,
        }
    }
}

fn parse_answers(packet: &[u8]) -> io::Result<Vec<IpAddr>> {
    let bad = || io::Error::other("malformed DNS answer");
    let flags = u16::from_be_bytes([packet[2], packet[3]]);
    if flags & 0x8000 == 0 || flags & 0x0200 != 0 || flags & 0x000f != 0 {
        return Err(io::Error::other(
            "DNS server refused or truncated the answer",
        ));
    }
    let questions = u16::from_be_bytes([packet[4], packet[5]]);
    let answers = u16::from_be_bytes([packet[6], packet[7]]);
    let mut at = 12;
    for _ in 0..questions {
        at = skip_name(packet, at).ok_or_else(bad)? + 4;
    }
    let mut found = Vec::new();
    for _ in 0..answers {
        at = skip_name(packet, at).ok_or_else(bad)?;
        let header = packet.get(at..at + 10).ok_or_else(bad)?;
        let kind = u16::from_be_bytes([header[0], header[1]]);
        let length = usize::from(u16::from_be_bytes([header[8], header[9]]));
        let data = packet.get(at + 10..at + 10 + length).ok_or_else(bad)?;
        match (kind, length) {
            (TYPE_A, 4) => found.push(IpAddr::V4(Ipv4Addr::new(
                data[0], data[1], data[2], data[3],
            ))),
            (TYPE_AAAA, 16) => {
                let mut octets = [0u8; 16];
                octets.copy_from_slice(data);
                found.push(IpAddr::V6(Ipv6Addr::from(octets)));
            }
            _ => {}
        }
        at += 10 + length;
    }
    Ok(found)
}

//! Which addresses an issuer fetch may never reach.

use crate::cidr::Cidr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn forbidden_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    a == 0
        || a == 10
        || (a == 100 && (64..128).contains(&b))
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224
}

fn forbidden_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let embedded = |hi: u16, lo: u16| {
        let [a, b] = hi.to_be_bytes();
        let [c, d] = lo.to_be_bytes();
        forbidden_v4(Ipv4Addr::new(a, b, c, d))
    };
    ip.is_unspecified()
        || ip.is_loopback()
        || (s[0] == 0x0064 && s[1] == 0xff9b && s[2..6] == [0; 4] && embedded(s[6], s[7]))
        || (s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 1)
        || (s[0] == 0x0100 && s[1..4] == [0; 3])
        || (s[0] == 0x2001 && s[1] < 0x0200)
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        || (s[0] == 0x2002 && embedded(s[1], s[2]))
        || s[0] == 0x5f00
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        || (s[0] & 0xff00) == 0xff00
}

/// Whether the address is loopback, private, link-local, CGNAT, unique-local,
/// multicast, reserved or a metadata address, looking through IPv4 embeddings.
pub fn is_forbidden(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => forbidden_v4(v4),
        IpAddr::V6(v6) => forbidden_v6(v6),
    }
}

/// Whether a fetch may connect to `ip`: it is public, or the operator allows its range.
pub fn permitted(ip: IpAddr, allowed: &[Cidr]) -> bool {
    let ip = ip.to_canonical();
    allowed.iter().any(|range| range.contains(ip)) || !is_forbidden(ip)
}

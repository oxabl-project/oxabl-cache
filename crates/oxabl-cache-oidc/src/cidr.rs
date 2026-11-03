//! Address ranges the operator allows an issuer to resolve into.

use std::net::IpAddr;

/// An address prefix such as `10.99.0.0/16`; a bare address is a single-host range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    network: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Parses `address` or `address/prefix`; the host bits must be zero.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (address, prefix) = match text.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (text, None),
        };
        let network: IpAddr = address
            .parse()
            .map_err(|_| format!("`{text}` is not an address range"))?;
        let width = if network.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => width,
            Some(prefix) => prefix
                .parse::<u8>()
                .ok()
                .filter(|prefix| *prefix <= width)
                .ok_or_else(|| format!("`{text}` has an invalid prefix length"))?,
        };
        let cidr = Self { network, prefix };
        if cidr.masked(network) != network {
            return Err(format!("`{text}` has host bits set"));
        }
        Ok(cidr)
    }

    fn masked(&self, address: IpAddr) -> IpAddr {
        match address {
            IpAddr::V4(v4) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                IpAddr::V4((u32::from(v4) & mask).into())
            }
            IpAddr::V6(v6) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                IpAddr::V6((u128::from(v6) & mask).into())
            }
        }
    }

    /// Whether `address` lies in the range; families never mix.
    pub fn contains(&self, address: IpAddr) -> bool {
        self.network.is_ipv4() == address.is_ipv4() && self.masked(address) == self.network
    }
}

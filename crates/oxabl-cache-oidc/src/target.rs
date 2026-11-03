//! The URLs an issuer fetch may name.

use crate::address::permitted;
use crate::cidr::Cidr;
use std::net::IpAddr;
use url::{Host, Url};

/// A validated `https` URL on port 443 without credentials.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub host: Host<String>,
    pub path_and_query: String,
}

impl Target {
    /// Parses an absolute URL; anything but https on 443 is refused. An IP literal is
    /// checked here, a name after resolution.
    pub fn parse(text: &str, allowed: &[Cidr]) -> Result<Self, &'static str> {
        let url = Url::parse(text).map_err(|_| "malformed URL")?;
        if url.scheme() != "https" {
            return Err("only https is allowed");
        }
        if url.port().is_some() {
            return Err("only port 443 is allowed");
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err("credentials in a URL are refused");
        }
        let host = url.host().ok_or("the URL has no host")?.to_owned();
        match &host {
            Host::Domain(name) if name.is_empty() => return Err("the URL has no host"),
            Host::Ipv4(ip) if !permitted(IpAddr::V4(*ip), allowed) => {
                return Err("the address is not allowed");
            }
            Host::Ipv6(ip) if !permitted(IpAddr::V6(*ip), allowed) => {
                return Err("the address is not allowed");
            }
            _ => {}
        }
        let mut path_and_query = url.path().to_owned();
        if let Some(query) = url.query() {
            path_and_query.push('?');
            path_and_query.push_str(query);
        }
        Ok(Self {
            host,
            path_and_query,
        })
    }

    /// The `Host` header value and the name TLS verifies the certificate for.
    pub fn authority(&self) -> String {
        match &self.host {
            Host::Domain(name) => name.clone(),
            Host::Ipv4(ip) => ip.to_string(),
            Host::Ipv6(ip) => format!("[{ip}]"),
        }
    }

    /// The URL of `location` as a redirect from this target names it.
    pub fn join(&self, location: &str) -> Option<String> {
        let base = Url::parse(&format!(
            "https://{}{}",
            self.authority(),
            self.path_and_query
        ))
        .ok()?;
        base.join(location).ok().map(String::from)
    }
}

use url::Url;

use super::error::ConfigError;

/// Accepts `https`, and `http` only on a loopback address.
pub(crate) fn check_server(url: &Url) -> Result<(), ConfigError> {
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name == "localhost",
        None => false,
    };
    match url.scheme() {
        "https" => Ok(()),
        "http" if loopback => Ok(()),
        _ => Err(ConfigError::InsecureServer),
    }
}

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use url::Url;

use super::LoginError;
use super::endpoint::check_endpoint;

const BODY_CAP: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// The two endpoints of the device grant, from the issuer's discovery document.
pub(super) struct Endpoints {
    pub device: Url,
    pub token: Url,
}

#[derive(Deserialize)]
struct Document {
    issuer: String,
    device_authorization_endpoint: Option<String>,
    token_endpoint: Option<String>,
}

pub(super) async fn read_capped(response: reqwest::Response) -> Result<Vec<u8>, LoginError> {
    if response
        .content_length()
        .is_some_and(|n| n > BODY_CAP as u64)
    {
        return Err(LoginError::Protocol("the answer is too large"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| LoginError::Unreachable)?;
    if bytes.len() > BODY_CAP {
        return Err(LoginError::Protocol("the answer is too large"));
    }
    Ok(bytes.to_vec())
}

/// Reads `<issuer>/.well-known/openid-configuration`. The document must name the issuer that
/// was asked for, and both endpoints must be `https` (or loopback `http`).
pub(super) async fn discover(http: &Client, issuer: &Url) -> Result<Endpoints, LoginError> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.as_str().trim_end_matches('/')
    );
    let response = http
        .get(url)
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|_| LoginError::Unreachable)?;
    if !response.status().is_success() {
        return Err(LoginError::Issuer("discovery failed"));
    }
    let document: Document = serde_json::from_slice(&read_capped(response).await?)
        .map_err(|_| LoginError::Issuer("discovery document is not valid"))?;
    if document.issuer.trim_end_matches('/') != issuer.as_str().trim_end_matches('/') {
        return Err(LoginError::Issuer("the document names another issuer"));
    }
    let endpoint = |value: Option<String>, what: &'static str| {
        let url = Url::parse(&value.ok_or(LoginError::Issuer(what))?)
            .map_err(|_| LoginError::Issuer(what))?;
        check_endpoint(&url).map_err(|_| LoginError::Issuer(what))?;
        Ok::<_, LoginError>(url)
    };
    Ok(Endpoints {
        device: endpoint(
            document.device_authorization_endpoint,
            "the issuer has no device authorization endpoint",
        )?,
        token: endpoint(document.token_endpoint, "the issuer has no token endpoint")?,
    })
}

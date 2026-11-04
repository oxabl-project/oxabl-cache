//! The device authorization grant of the fake identity provider: a device endpoint and a
//! token endpoint that answer like an issuer whose user approves after a few polls.

use super::keys::SigningKey;
use super::tokens;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

/// How the issuer behaves for one `/device` and `/token` pair.
pub struct Device {
    /// The client id the issuer accepts.
    pub client_id: String,
    /// Signs the ID tokens it returns.
    pub key: Arc<SigningKey>,
    /// The `sub` of every ID token.
    pub subject: String,
    /// The `aud` of every ID token; `None` mints the audience the client asked for.
    pub audience: Option<String>,
    /// Polls that are answered `authorization_pending` before the approval.
    pub pending_polls: usize,
    /// Answers the first poll `slow_down` once.
    pub slow_down: bool,
    /// Answers every poll `access_denied`.
    pub deny: bool,
}

struct Waiting {
    audience: String,
    polls: usize,
}

/// A configured device grant and the logins it is serving.
pub(super) struct DeviceGrant {
    device: Device,
    waiting: HashMap<String, Waiting>,
    pub requests: Vec<String>,
    slowed: bool,
    issued: usize,
}

fn field(body: &str, name: &str) -> Option<String> {
    url::form_urlencoded::parse(body.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn refusal(error: &str) -> (&'static str, Value) {
    ("400 Bad Request", json!({ "error": error }))
}

impl DeviceGrant {
    pub fn new(device: Device) -> Self {
        Self {
            device,
            waiting: HashMap::new(),
            requests: Vec::new(),
            slowed: false,
            issued: 0,
        }
    }

    /// `POST <prefix>/device`.
    pub fn authorize(&mut self, base: &str, body: &str) -> (&'static str, Value) {
        self.requests.push(body.to_owned());
        if field(body, "client_id").as_deref() != Some(&self.device.client_id) {
            return refusal("invalid_client");
        }
        self.issued += 1;
        let device_code = format!("device-code-{}", self.issued);
        self.waiting.insert(
            device_code.clone(),
            Waiting {
                audience: field(body, "audience").unwrap_or_default(),
                polls: 0,
            },
        );
        (
            "200 OK",
            json!({
                "device_code": device_code,
                "user_code": "WDJB-MJHT",
                "verification_uri": format!("{base}/activate"),
                "verification_uri_complete": format!("{base}/activate?user_code=WDJB-MJHT"),
                "expires_in": 600,
                "interval": 1,
            }),
        )
    }

    /// `POST <prefix>/token`.
    pub fn token(&mut self, issuer: &str, body: &str) -> (&'static str, Value) {
        let grant = field(body, "grant_type");
        if grant.as_deref() != Some("urn:ietf:params:oauth:grant-type:device_code")
            || field(body, "client_id").as_deref() != Some(&self.device.client_id)
        {
            return refusal("invalid_request");
        }
        let Some(waiting) = field(body, "device_code").and_then(|c| self.waiting.get_mut(&c))
        else {
            return refusal("expired_token");
        };
        waiting.polls += 1;
        if self.device.deny {
            return refusal("access_denied");
        }
        if self.device.slow_down && !self.slowed {
            self.slowed = true;
            return refusal("slow_down");
        }
        if waiting.polls <= self.device.pending_polls {
            return refusal("authorization_pending");
        }
        let audience = self
            .device
            .audience
            .clone()
            .unwrap_or_else(|| waiting.audience.clone());
        let claims = tokens::generic(issuer, &audience, &self.device.subject);
        (
            "200 OK",
            json!({ "id_token": tokens::sign(&self.device.key, &claims), "token_type": "Bearer" }),
        )
    }
}

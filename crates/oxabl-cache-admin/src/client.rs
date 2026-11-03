//! The HTTP client for the admin listener: bearer token, error envelope, exit mapping.

use crate::config::Settings;
use crate::error::Failure;
use reqwest::blocking::{Client, Response};
use reqwest::header::{AUTHORIZATION, HeaderValue};
use reqwest::{Certificate, Method, redirect};
use serde_json::Value;
use std::fs;
use std::time::Duration;

pub struct Admin {
    http: Client,
    server: String,
    auth: HeaderValue,
}

impl Admin {
    pub fn new(settings: Settings) -> Result<Self, Failure> {
        let mut builder = Client::builder()
            .redirect(redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(None);
        if let Some(path) = &settings.ca_file {
            let pem = fs::read(path).map_err(|error| {
                Failure::usage(format!("cannot read CA file {}: {error}", path.display()))
            })?;
            let certs = Certificate::from_pem_bundle(&pem).map_err(|error| {
                Failure::usage(format!("invalid CA file {}: {error}", path.display()))
            })?;
            if certs.is_empty() {
                return Err(Failure::usage(format!(
                    "CA file {} holds no certificate",
                    path.display()
                )));
            }
            for cert in certs {
                builder = builder.add_root_certificate(cert);
            }
        }
        let http = builder
            .build()
            .map_err(|error| Failure::usage(format!("cannot build the HTTP client: {error}")))?;
        let mut auth = HeaderValue::from_str(&format!("Bearer {}", settings.token))
            .map_err(|_| Failure::usage("the token holds characters a header cannot carry"))?;
        auth.set_sensitive(true);
        Ok(Self {
            http,
            server: settings.server,
            auth,
        })
    }

    fn send(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Response, Failure> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.server))
            .header(AUTHORIZATION, self.auth.clone());
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .map_err(|error| Failure::Unreachable(describe(error)))?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(refusal(response))
        }
    }

    pub fn json(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value, Failure> {
        let response = self.send(method, path, body)?;
        let status = response.status().as_u16();
        let bytes = response
            .bytes()
            .map_err(|error| Failure::Unreachable(describe(error)))?;
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            Failure::refused(
                status,
                "bad_response",
                "the server answered with invalid JSON",
            )
        })
    }

    pub fn text(&self, path: &str) -> Result<String, Failure> {
        self.send(Method::GET, path, None)?
            .text()
            .map_err(|error| Failure::Unreachable(describe(error)))
    }
}

fn describe(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut text = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(inner) = source {
        text.push_str(": ");
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    text
}

fn refusal(response: Response) -> Failure {
    let status = response.status().as_u16();
    let body: Value = response.json().unwrap_or(Value::Null);
    let error = &body["error"];
    match (error["code"].as_str(), error["message"].as_str()) {
        (Some(code), Some(message)) => Failure::refused(status, code, message),
        _ => Failure::refused(
            status,
            "http_error",
            format!("the server answered {status}"),
        ),
    }
}

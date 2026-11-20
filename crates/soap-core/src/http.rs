//! Building and sending SOAP HTTP requests.

use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

use crate::envelope::{WsSecurity, apply_ws_security};
use crate::wsdl::SoapVersion;
use crate::{Error, Result, xml_fmt};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Auth {
    #[default]
    None,
    Basic { username: String, password: String },
    WsSecurity(WsSecurity),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSpec {
    pub endpoint: String,
    pub soap_version: SoapVersion,
    #[serde(default)]
    pub soap_action: Option<String>,
    pub body: String,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub accept_invalid_certs: bool,
}

fn default_timeout() -> u64 {
    60
}

/// The exact request that goes on the wire (after auth/WS-Security are applied).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseData {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// Pretty-printed body, if it is XML.
    pub body_pretty: Option<String>,
    pub elapsed_ms: u64,
    pub size_bytes: usize,
    pub request: PreparedRequest,
}

/// Turn a request spec into concrete URL/headers/body. Pure, so it is easy to test.
pub fn prepare(spec: &RequestSpec) -> Result<PreparedRequest> {
    let url = spec.endpoint.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(Error::Http(format!("endpoint must start with http:// or https://, got '{url}'")));
    }

    let mut body = spec.body.clone();
    let mut headers: Vec<(String, String)> = Vec::new();
    let action = spec.soap_action.clone().unwrap_or_default();
    match spec.soap_version {
        SoapVersion::Soap11 => {
            headers.push(("Content-Type".into(), "text/xml;charset=UTF-8".into()));
            headers.push(("SOAPAction".into(), format!("\"{action}\"")));
        }
        SoapVersion::Soap12 => {
            let mut ct = "application/soap+xml;charset=UTF-8".to_string();
            if !action.is_empty() {
                ct.push_str(&format!(";action=\"{action}\""));
            }
            headers.push(("Content-Type".into(), ct));
        }
    }

    match &spec.auth {
        Auth::None => {}
        Auth::Basic { username, password } => {
            let token = B64.encode(format!("{username}:{password}"));
            headers.push(("Authorization".into(), format!("Basic {token}")));
        }
        Auth::WsSecurity(sec) => body = apply_ws_security(&body, sec)?,
    }

    // User headers override generated ones with the same name.
    for h in spec.headers.iter().filter(|h| h.enabled && !h.name.trim().is_empty()) {
        headers.retain(|(n, _)| !n.eq_ignore_ascii_case(h.name.trim()));
        headers.push((h.name.trim().to_string(), h.value.clone()));
    }

    Ok(PreparedRequest { url, headers, body })
}

pub async fn send(spec: &RequestSpec) -> Result<ResponseData> {
    let prepared = prepare(spec)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(spec.timeout_secs.max(1)))
        .tls_danger_accept_invalid_certs(spec.accept_invalid_certs)
        .user_agent(concat!("soap-client/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let mut req = client.post(&prepared.url).body(prepared.body.clone());
    for (n, v) in &prepared.headers {
        req = req.header(n, v);
    }

    let start = Instant::now();
    let resp = req.send().await?;
    let status = resp.status();
    let headers = resp
        .headers()
        .iter()
        .map(|(n, v)| (n.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let bytes = resp.bytes().await?;
    let elapsed_ms = start.elapsed().as_millis() as u64;
    let body = String::from_utf8_lossy(&bytes).into_owned();

    Ok(ResponseData {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        headers,
        body_pretty: xml_fmt::pretty_print(&body),
        body,
        elapsed_ms,
        size_bytes: bytes.len(),
        request: prepared,
    })
}

/// Download a document (e.g. a WSDL) over HTTP(S).
pub async fn fetch_text(url: &str, accept_invalid_certs: bool) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .tls_danger_accept_invalid_certs(accept_invalid_certs)
        .build()?;
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Http(format!("GET {url} returned {status}")));
    }
    Ok(resp.text().await?)
}

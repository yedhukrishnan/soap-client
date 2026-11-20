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
    if url.is_empty() {
        return Err(Error::Http("enter an endpoint URL".into()));
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(Error::Http(format!("the endpoint must start with http:// or https:// (got '{url}')")));
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

pub const MASK: &str = "••••••";

/// Copy of a prepared request that is safe to display: auth passwords, the
/// Authorization header and any extra `secrets` (raw or XML-escaped) are masked.
pub fn masked(prepared: &PreparedRequest, spec: &RequestSpec, secrets: &[String]) -> PreparedRequest {
    let mut values: Vec<String> = secrets.to_vec();
    match &spec.auth {
        Auth::Basic { password, .. } => values.push(password.clone()),
        Auth::WsSecurity(sec) => values.push(sec.password.clone()),
        Auth::None => {}
    }
    // Longest first so a secret containing another secret is masked whole.
    values.retain(|v| v.chars().count() >= 2);
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    values.dedup();
    let hide = |s: &str| {
        values.iter().fold(s.to_string(), |acc, v| {
            acc.replace(v.as_str(), MASK).replace(&crate::xml_escape(v), MASK)
        })
    };
    PreparedRequest {
        url: hide(&prepared.url),
        headers: prepared
            .headers
            .iter()
            .map(|(n, v)| {
                let v = if n.eq_ignore_ascii_case("authorization") {
                    match v.split_once(' ') {
                        Some((scheme, _)) => format!("{scheme} {MASK}"),
                        None => MASK.to_string(),
                    }
                } else {
                    hide(v)
                };
                (n.clone(), v)
            })
            .collect(),
        body: hide(&prepared.body),
    }
}

/// Send a request. `secrets` are values (e.g. secret variables) that must be masked
/// in the returned copy of the sent request.
/// Largest response body accepted.
pub const MAX_RESPONSE_BYTES: u64 = 100 * 1024 * 1024;

/// Read a body, failing once it exceeds `limit` bytes (instead of exhausting memory).
async fn read_limited(mut resp: reqwest::Response, limit: u64) -> Result<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > limit) {
        return Err(Error::Http(format!("response is larger than {} MB", limit >> 20)));
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() as u64 + chunk.len() as u64 > limit {
            return Err(Error::Http(format!("response is larger than {} MB", limit >> 20)));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Most redirects followed within one origin.
const MAX_REDIRECTS: usize = 10;

/// Follow redirects only within the origin (scheme, host and port) of the request. A
/// redirect anywhere else is returned as the response: following it would send custom
/// headers and, for 307/308, the body (WS-Security passwords, secret variables) to
/// another server, possibly over plain http.
fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let same_origin = attempt.previous().first().is_some_and(|first| first.origin() == attempt.url().origin());
        if !same_origin {
            attempt.stop()
        } else if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

pub async fn send(spec: &RequestSpec, secrets: &[String]) -> Result<ResponseData> {
    let prepared = prepare(spec)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(spec.timeout_secs.max(1)))
        .tls_danger_accept_invalid_certs(spec.accept_invalid_certs)
        .user_agent(concat!("soap-client/", env!("CARGO_PKG_VERSION")))
        .redirect(same_origin_redirects())
        // The endpoint URL may contain secret variables; never pass it on as a Referer.
        .referer(false)
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
    let bytes = read_limited(resp, MAX_RESPONSE_BYTES).await?;
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
        request: masked(&prepared, spec, secrets),
    })
}

/// Whether a redirect to `next` would leave https for plain http at any point in the chain.
fn is_downgrade(previous: &[reqwest::Url], next: &reqwest::Url) -> bool {
    next.scheme() == "http" && previous.iter().any(|u| u.scheme() == "https")
}

/// Document downloads are plain GETs without a body or custom headers, so redirects to
/// other servers are followed (http to https, `www.`, CDNs are common for WSDLs), but
/// never from https to http, where the document could be altered on the way.
fn document_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if is_downgrade(attempt.previous(), attempt.url()) {
            attempt.error("refusing to follow a redirect from https to http")
        } else if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

/// Download a document (e.g. a WSDL) over HTTP(S).
pub async fn fetch_text(url: &str, accept_invalid_certs: bool, limit: u64) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .tls_danger_accept_invalid_certs(accept_invalid_certs)
        .redirect(document_redirects())
        // The URL may carry a token in its query; don't pass it on to the redirect target.
        .referer(false)
        .build()?;
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Http(format!("GET {url} returned {status}")));
    }
    let bytes = read_limited(resp, limit).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::is_downgrade;
    use reqwest::Url;

    fn urls(list: &[&str]) -> Vec<Url> {
        list.iter().map(|u| Url::parse(u).unwrap()).collect()
    }

    #[test]
    fn downgrade_is_any_https_to_http_step_in_the_chain() {
        let http = Url::parse("http://b.example/x.wsdl").unwrap();
        let https = Url::parse("https://b.example/x.wsdl").unwrap();
        assert!(is_downgrade(&urls(&["https://a.example/x.wsdl"]), &http));
        assert!(is_downgrade(&urls(&["http://a.example/", "https://a.example/"]), &http));
        assert!(!is_downgrade(&urls(&["http://a.example/x.wsdl"]), &http));
        assert!(!is_downgrade(&urls(&["http://a.example/x.wsdl"]), &https));
        assert!(!is_downgrade(&urls(&["https://a.example/x.wsdl"]), &https));
    }
}

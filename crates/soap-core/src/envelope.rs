//! SOAP envelope helpers: version detection and WS-Security header injection.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{Duration, SecondsFormat, Utc};
use roxmltree::Document;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::wsdl::SoapVersion;
use crate::{Error, NS_SOAP11_ENV, NS_SOAP12_ENV, NS_WSSE, NS_WSU, Result, xml_escape};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasswordType {
    #[default]
    PasswordText,
    PasswordDigest,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WsSecurity {
    pub username: String,
    pub password: String,
    pub password_type: PasswordType,
    /// Add a wsu:Timestamp with this time-to-live in seconds (0 = no timestamp).
    pub timestamp_ttl_secs: u32,
}

/// Detect the SOAP version from the envelope's root namespace.
pub fn detect_version(envelope: &str) -> Option<SoapVersion> {
    let doc = Document::parse(envelope).ok()?;
    match doc.root_element().tag_name().namespace()? {
        NS_SOAP11_ENV => Some(SoapVersion::Soap11),
        NS_SOAP12_ENV => Some(SoapVersion::Soap12),
        _ => None,
    }
}

/// Insert a `wsse:Security` header into an envelope, creating `Header` if necessary.
/// The edit is textual so the user's formatting and prefixes are preserved.
pub fn apply_ws_security(envelope: &str, sec: &WsSecurity) -> Result<String> {
    let doc = Document::parse(envelope)?;
    let root = doc.root_element();
    let env_ns = root.tag_name().namespace().unwrap_or("");
    if root.tag_name().name() != "Envelope" || !(env_ns == NS_SOAP11_ENV || env_ns == NS_SOAP12_ENV) {
        return Err(Error::Xml("the request body is not a SOAP Envelope".into()));
    }
    let prefix = root.lookup_prefix(env_ns).unwrap_or("");
    let q = |local: &str| if prefix.is_empty() { local.to_string() } else { format!("{prefix}:{local}") };
    let must_understand = if env_ns == NS_SOAP12_ENV { "true" } else { "1" };
    // Bind the envelope namespace explicitly in case the envelope uses a default namespace.
    let mu_attr = if prefix.is_empty() {
        format!(" xmlns:env=\"{env_ns}\" env:mustUnderstand=\"{must_understand}\"")
    } else {
        format!(" {prefix}:mustUnderstand=\"{must_understand}\"")
    };
    let security = security_xml(sec, &mu_attr, Utc::now(), &random_nonce());

    let child = |local: &str| {
        root.children().find(|c| {
            c.is_element() && c.tag_name().namespace() == Some(env_ns) && c.tag_name().name() == local
        })
    };

    let mut out = envelope.to_string();
    if let Some(header) = child("Header") {
        let r = header.range();
        let src = &envelope[r.clone()];
        if src.trim_end().ends_with("/>") {
            out.replace_range(r, &format!("<{h}>{security}</{h}>", h = q("Header")));
        } else {
            let close = src.rfind("</").ok_or_else(|| Error::Xml("the SOAP Header element is malformed".into()))?;
            out.insert_str(r.start + close, &security);
        }
    } else if let Some(body) = child("Body") {
        out.insert_str(body.range().start, &format!("<{h}>{security}</{h}>", h = q("Header")));
    } else {
        return Err(Error::Xml("the SOAP Envelope has no Body".into()));
    }
    Ok(out)
}

fn random_nonce() -> [u8; 16] {
    rand::random()
}

pub(crate) fn security_xml(
    sec: &WsSecurity,
    mu_attr: &str,
    now: chrono::DateTime<Utc>,
    nonce: &[u8],
) -> String {
    let created = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let (pw_type, pw) = match sec.password_type {
        PasswordType::PasswordText => ("PasswordText", xml_escape(&sec.password)),
        PasswordType::PasswordDigest => {
            let mut h = Sha1::new();
            h.update(nonce);
            h.update(created.as_bytes());
            h.update(sec.password.as_bytes());
            ("PasswordDigest", B64.encode(h.finalize()))
        }
    };
    let token_profile = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0";
    let soap_msg_sec = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0";

    let mut s = format!("<wsse:Security{mu_attr} xmlns:wsse=\"{NS_WSSE}\" xmlns:wsu=\"{NS_WSU}\">");
    if sec.timestamp_ttl_secs > 0 {
        let expires = (now + Duration::seconds(sec.timestamp_ttl_secs as i64))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        s.push_str(&format!(
            "<wsu:Timestamp wsu:Id=\"TS-{}\"><wsu:Created>{created}</wsu:Created><wsu:Expires>{expires}</wsu:Expires></wsu:Timestamp>",
            uuid::Uuid::new_v4().simple()
        ));
    }
    s.push_str(&format!(
        "<wsse:UsernameToken wsu:Id=\"UsernameToken-{}\"><wsse:Username>{}</wsse:Username>\
<wsse:Password Type=\"{token_profile}#{pw_type}\">{pw}</wsse:Password>\
<wsse:Nonce EncodingType=\"{soap_msg_sec}#Base64Binary\">{}</wsse:Nonce>\
<wsu:Created>{created}</wsu:Created></wsse:UsernameToken></wsse:Security>",
        uuid::Uuid::new_v4().simple(),
        xml_escape(&sec.username),
        B64.encode(nonce),
    ));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: fn() -> WsSecurity = || WsSecurity {
        username: "bob".into(),
        password: "p&ss".into(),
        ..Default::default()
    };

    fn assert_has_security(out: &str) {
        let doc = Document::parse(out).expect("result must be well-formed");
        let sec = doc
            .descendants()
            .find(|n| n.tag_name().name() == "Security" && n.tag_name().namespace() == Some(NS_WSSE))
            .expect("Security element");
        assert_eq!(sec.parent().unwrap().tag_name().name(), "Header");
        assert!(out.contains("<wsse:Username>bob</wsse:Username>"));
        assert!(out.contains(">p&amp;ss</wsse:Password>"));
    }

    #[test]
    fn injects_into_self_closing_header() {
        let env = r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/"><soapenv:Header/><soapenv:Body><a/></soapenv:Body></soapenv:Envelope>"#;
        let out = apply_ws_security(env, &SEC()).unwrap();
        assert_has_security(&out);
        assert!(out.contains("soapenv:mustUnderstand=\"1\""));
    }

    #[test]
    fn injects_into_existing_header_and_keeps_children() {
        let env = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope"><s:Header><x>1</x></s:Header><s:Body/></s:Envelope>"#;
        let out = apply_ws_security(env, &SEC()).unwrap();
        assert_has_security(&out);
        assert!(out.contains("<x>1</x><wsse:Security s:mustUnderstand=\"true\""));
    }

    #[test]
    fn creates_header_when_missing() {
        let env = r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/"><soapenv:Body/></soapenv:Envelope>"#;
        assert_has_security(&apply_ws_security(env, &SEC()).unwrap());
    }

    #[test]
    fn default_namespace_envelope() {
        let env = r#"<Envelope xmlns="http://schemas.xmlsoap.org/soap/envelope/"><Body/></Envelope>"#;
        assert_has_security(&apply_ws_security(env, &SEC()).unwrap());
    }

    #[test]
    fn digest_matches_spec_formula() {
        let sec = WsSecurity { password_type: PasswordType::PasswordDigest, ..SEC() };
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().to_utc();
        let nonce = [7u8; 16];
        let xml = security_xml(&sec, "", now, &nonce);
        let mut h = Sha1::new();
        h.update(nonce);
        h.update(b"2026-01-01T00:00:00.000Z");
        h.update(b"p&ss");
        assert!(xml.contains(&format!("#PasswordDigest\">{}<", B64.encode(h.finalize()))));
    }

    #[test]
    fn rejects_non_envelope() {
        assert!(apply_ws_security("<foo/>", &SEC()).is_err());
    }
}

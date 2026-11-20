//! Response assertions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sxd_xpath::{Context, Factory, Value};

use crate::http::ResponseData;
use crate::{NS_SOAP11_ENV, NS_SOAP12_ENV};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Check {
    StatusCode { code: u16 },
    #[serde(rename_all = "camelCase")]
    Contains { text: String, #[serde(default)] ignore_case: bool },
    #[serde(rename_all = "camelCase")]
    NotContains { text: String, #[serde(default)] ignore_case: bool },
    /// XPath 1.0. Namespace prefixes declared anywhere in the response can be used,
    /// plus `soap11`/`soap12` for the envelope namespaces.
    /// With an empty `expected`, passes when the expression matches anything.
    #[serde(rename = "xpath")]
    XPathMatch { expression: String, #[serde(default)] expected: String },
    NoSoapFault,
    MaxResponseTime { ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assertion {
    pub id: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub check: Check,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionResult {
    pub id: String,
    pub passed: bool,
    pub message: String,
}

pub fn evaluate_all(assertions: &[Assertion], resp: &ResponseData) -> Vec<AssertionResult> {
    assertions
        .iter()
        .filter(|a| a.enabled)
        .map(|a| {
            let (passed, message) = evaluate(&a.check, resp);
            AssertionResult { id: a.id.clone(), passed, message }
        })
        .collect()
}

pub fn evaluate(check: &Check, resp: &ResponseData) -> (bool, String) {
    match check {
        Check::StatusCode { code } => (
            resp.status == *code,
            format!("expected status {code}, got {}", resp.status),
        ),
        Check::Contains { text, ignore_case } => {
            let found = contains(&resp.body, text, *ignore_case);
            (found, if found { format!("found \"{text}\"") } else { format!("\"{text}\" not found") })
        }
        Check::NotContains { text, ignore_case } => {
            let found = contains(&resp.body, text, *ignore_case);
            (!found, if found { format!("\"{text}\" was found") } else { format!("\"{text}\" absent") })
        }
        Check::NoSoapFault => match find_fault(&resp.body) {
            Some(msg) => (false, format!("SOAP Fault: {msg}")),
            None => (true, "no SOAP Fault".into()),
        },
        Check::MaxResponseTime { ms } => (
            resp.elapsed_ms <= *ms,
            format!("took {} ms (limit {ms} ms)", resp.elapsed_ms),
        ),
        Check::XPathMatch { expression, expected } => match xpath_value(&resp.body, expression) {
            Err(e) => (false, e),
            Ok(XPathOutcome { matched, value }) => {
                if expected.trim().is_empty() {
                    (matched, if matched { format!("matched: {value}") } else { "no match".into() })
                } else {
                    let ok = matched && value.trim() == expected.trim();
                    (ok, format!("expected \"{}\", got \"{}\"", expected.trim(), value.trim()))
                }
            }
        },
    }
}

fn contains(hay: &str, needle: &str, ignore_case: bool) -> bool {
    if ignore_case {
        hay.to_lowercase().contains(&needle.to_lowercase())
    } else {
        hay.contains(needle)
    }
}

/// Returns the faultstring / Reason text if the body contains a SOAP Fault.
fn find_fault(body: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(body).ok()?;
    let fault = doc.descendants().find(|n| {
        n.is_element()
            && n.tag_name().name() == "Fault"
            && matches!(n.tag_name().namespace(), Some(NS_SOAP11_ENV) | Some(NS_SOAP12_ENV))
    })?;
    let text = fault
        .descendants()
        .find(|n| n.is_element() && matches!(n.tag_name().name(), "faultstring" | "Text"))
        .and_then(|n| n.text())
        .unwrap_or("(no message)");
    Some(text.trim().to_string())
}

struct XPathOutcome {
    matched: bool,
    value: String,
}

fn xpath_value(body: &str, expr: &str) -> std::result::Result<XPathOutcome, String> {
    let package = sxd_document::parser::parse(body).map_err(|e| format!("response is not XML: {e:?}"))?;
    let doc = package.as_document();
    let xpath = Factory::new()
        .build(expr)
        .map_err(|e| format!("invalid XPath: {e}"))?
        .ok_or("empty XPath expression")?;

    let mut ctx = Context::new();
    for (prefix, uri) in response_namespaces(body) {
        ctx.set_namespace(&prefix, &uri);
    }
    let value = xpath.evaluate(&ctx, doc.root()).map_err(|e| format!("XPath error: {e}"))?;
    Ok(match value {
        Value::Nodeset(ns) => match ns.document_order_first() {
            Some(n) => XPathOutcome { matched: true, value: n.string_value() },
            None => XPathOutcome { matched: false, value: String::new() },
        },
        Value::Boolean(b) => XPathOutcome { matched: b, value: b.to_string() },
        Value::Number(n) => XPathOutcome { matched: !n.is_nan(), value: fmt_number(n) },
        Value::String(s) => XPathOutcome { matched: !s.is_empty(), value: s },
    })
}

fn fmt_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { n.to_string() }
}

/// Every prefixed namespace declared in the document (first declaration wins).
fn response_namespaces(body: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("soap11".to_string(), NS_SOAP11_ENV.to_string());
    map.insert("soap12".to_string(), NS_SOAP12_ENV.to_string());
    if let Ok(doc) = roxmltree::Document::parse(body) {
        for n in doc.descendants().filter(|n| n.is_element()) {
            for ns in n.namespaces() {
                if let Some(p) = ns.name() {
                    map.entry(p.to_string()).or_insert_with(|| ns.uri().to_string());
                }
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::PreparedRequest;

    fn resp(status: u16, body: &str) -> ResponseData {
        ResponseData {
            status,
            status_text: String::new(),
            headers: vec![],
            body: body.into(),
            body_pretty: None,
            elapsed_ms: 120,
            size_bytes: body.len(),
            request: PreparedRequest { url: String::new(), headers: vec![], body: String::new() },
        }
    }

    const OK: &str = r#"<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body><AddResponse xmlns="http://tempuri.org/"><AddResult>5</AddResult></AddResponse></soap:Body></soap:Envelope>"#;
    const FAULT: &str = r#"<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body><soap:Fault><faultcode>soap:Client</faultcode><faultstring>Bad input</faultstring></soap:Fault></soap:Body></soap:Envelope>"#;

    #[test]
    fn status_and_text() {
        let r = resp(200, OK);
        assert!(evaluate(&Check::StatusCode { code: 200 }, &r).0);
        assert!(!evaluate(&Check::StatusCode { code: 500 }, &r).0);
        assert!(evaluate(&Check::Contains { text: "addresult".into(), ignore_case: true }, &r).0);
        assert!(!evaluate(&Check::Contains { text: "addresult".into(), ignore_case: false }, &r).0);
        assert!(evaluate(&Check::NotContains { text: "Fault".into(), ignore_case: false }, &r).0);
        assert!(evaluate(&Check::MaxResponseTime { ms: 200 }, &r).0);
        assert!(!evaluate(&Check::MaxResponseTime { ms: 100 }, &r).0);
    }

    #[test]
    fn soap_fault() {
        assert!(evaluate(&Check::NoSoapFault, &resp(200, OK)).0);
        let (ok, msg) = evaluate(&Check::NoSoapFault, &resp(500, FAULT));
        assert!(!ok);
        assert!(msg.contains("Bad input"));
    }

    #[test]
    fn xpath() {
        let r = resp(200, OK);
        let x = |e: &str, v: &str| evaluate(&Check::XPathMatch { expression: e.into(), expected: v.into() }, &r);
        assert!(x("//*[local-name()='AddResult']", "5").0);
        assert!(x("count(//soap:Body)", "1").0);
        assert!(x("//soap11:Body", "").0);
        assert!(!x("//*[local-name()='AddResult']", "6").0);
        assert!(!x("//Nope", "").0);
        assert!(!x("//[", "").0);
    }
}

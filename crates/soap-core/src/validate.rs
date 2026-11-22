//! Request validation: XML syntax, plus (optionally) conformance to the WSDL the
//! request was created from — envelope, SOAP version, SOAPAction, Body parts, and the
//! part contents against the XML Schema.
//!
//! Only syntax errors are severe enough to stop a request from being sent; everything
//! else is advisory. Validation runs on the request template, before `{{variables}}`
//! are substituted, so secret values never appear in messages.

use std::ops::Range;

use roxmltree::{Document, Node, ParsingOptions};
use serde::{Deserialize, Serialize};

use crate::wsdl::{ImportedDoc, Operation, SoapVersion};
use crate::xsd::validate::Validator;
use crate::xsd::{QName, SchemaIndex};
use crate::{NS_SOAP11_ENV, NS_SOAP12_ENV};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The body is not well-formed XML; the request cannot be sent.
    Error,
    /// The request does not match the WSDL; it can still be sent.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
    /// Position in the body as UTF-16 offsets (what the editor uses); `None` for issues
    /// that are not about the body text, e.g. the SOAPAction header.
    pub start: Option<u32>,
    pub end: Option<u32>,
}

/// What a request is checked against.
pub struct Target<'a> {
    pub wsdl_text: &'a str,
    pub imports: &'a [ImportedDoc],
    /// SOAP version of the WSDL binding.
    pub binding_version: SoapVersion,
    pub operation: &'a Operation,
    /// SOAP version the request is set to send with.
    pub soap_version: SoapVersion,
    pub soap_action: Option<&'a str>,
}

fn parse(text: &str) -> Result<Document<'_>, roxmltree::Error> {
    Document::parse_with_options(text, ParsingOptions { allow_dtd: true, ..Default::default() })
}

/// The syntax error in a request body, if it is not well-formed XML.
pub fn check_syntax(body: &str) -> Option<Issue> {
    parse(body).err().map(|e| syntax_issue(body, &e))
}

fn syntax_issue(body: &str, e: &roxmltree::Error) -> Issue {
    let pos = e.pos();
    // Row/column (1-based, column in characters) -> byte offset.
    let line_start: usize = body.split_inclusive('\n').take(pos.row.saturating_sub(1) as usize).map(str::len).sum();
    let line = &body[line_start.min(body.len())..];
    let col: usize = line.chars().take(pos.col.saturating_sub(1) as usize).map(char::len_utf8).sum();
    let start = (line_start + col).min(body.len());
    let end = body[start..].chars().next().map_or(start, |c| start + c.len_utf8());
    let message = format!("Not well-formed XML: {e}");
    let (s, en) = utf16_range(body, start..end);
    Issue { severity: Severity::Error, message, start: Some(s), end: Some(en) }
}

fn utf16_range(text: &str, r: Range<usize>) -> (u32, u32) {
    let s = text[..r.start].encode_utf16().count();
    let e = s + text[r.start..r.end].encode_utf16().count();
    (s as u32, e as u32)
}

fn version_name(v: SoapVersion) -> &'static str {
    match v {
        SoapVersion::Soap11 => "SOAP 1.1",
        SoapVersion::Soap12 => "SOAP 1.2",
    }
}

fn label(n: Node) -> String {
    format!("<{}>", n.tag_name().name())
}

fn tag_range(n: Node) -> Range<usize> {
    let text = n.document().input_text();
    let start = n.range().start;
    let rest = &text[start + 1..];
    let len = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len());
    start..start + 1 + len
}

fn qname_of(n: Node) -> QName {
    (n.tag_name().namespace().unwrap_or("").to_string(), n.tag_name().name().to_string())
}

fn ns_label(ns: &str) -> String {
    if ns.is_empty() { "no namespace".into() } else { format!("\"{ns}\"") }
}

struct Report<'b> {
    body: &'b str,
    issues: Vec<Issue>,
}

impl Report<'_> {
    fn warn(&mut self, range: Option<Range<usize>>, message: String) {
        let (start, end) = match range {
            Some(r) => {
                let (s, e) = utf16_range(self.body, r);
                (Some(s), Some(e))
            }
            None => (None, None),
        };
        self.issues.push(Issue { severity: Severity::Warning, message, start, end });
    }
}

/// Validate a request body. Without a target only syntax and `{{placeholder}}` spelling
/// are checked. `var_names` are the names of the defined variables (never their values).
pub fn validate_request(body: &str, target: Option<&Target>, var_names: &[String]) -> Vec<Issue> {
    let doc = match parse(body) {
        Ok(d) => d,
        Err(e) => return vec![syntax_issue(body, &e)],
    };
    let mut r = Report { body, issues: Vec::new() };
    // Template problems are reported whether or not the WSDL is checked.
    for range in crate::vars::malformed_placeholders(body) {
        let text = &body[range.clone()];
        r.warn(Some(range), format!("\"{text}\" is not a valid variable placeholder and would be sent as typed; use {{{{name}}}}"));
    }
    for (range, name) in crate::vars::single_brace_placeholders(body, var_names) {
        let text = &body[range.clone()];
        r.warn(Some(range), format!("\"{text}\" would be sent as typed: did you mean {{{{{name}}}}}?"));
    }
    r.issues.sort_by_key(|i| i.start);
    if let Some(t) = target {
        check_against(&doc, t, &mut r);
    }
    r.issues
}

fn check_against(doc: &Document, t: &Target, r: &mut Report) {
    let op = t.operation;

    // SOAPAction (sent as a header, so it has no position in the body).
    if let Some(expected) = op.soap_action.as_deref().filter(|a| !a.is_empty()) {
        let actual = t.soap_action.unwrap_or("").trim();
        if !actual.contains("{{") && actual != expected {
            let msg = if actual.is_empty() {
                format!("SOAPAction is empty; the WSDL specifies \"{expected}\" for {} (Headers tab)", op.name)
            } else {
                format!("SOAPAction is \"{actual}\", but the WSDL specifies \"{expected}\" for {} (Headers tab)", op.name)
            };
            r.warn(None, msg);
        }
    }

    let env = doc.root_element();
    let env_version = match env.tag_name().namespace() {
        Some(NS_SOAP11_ENV) => Some(SoapVersion::Soap11),
        Some(NS_SOAP12_ENV) => Some(SoapVersion::Soap12),
        _ => None,
    };
    let (Some(env_version), "Envelope") = (env_version, env.tag_name().name()) else {
        r.warn(Some(tag_range(env)), format!("The root element should be a SOAP Envelope, not {}", label(env)));
        return;
    };
    let env_ns = env_version.envelope_ns();
    if env_version != t.soap_version {
        r.warn(
            Some(tag_range(env)),
            format!(
                "This is a {} envelope, but the request is set to send {} (version selector next to the URL)",
                version_name(env_version),
                version_name(t.soap_version)
            ),
        );
    } else if env_version != t.binding_version {
        r.warn(
            Some(tag_range(env)),
            format!("The WSDL binding uses {}, but this is a {} envelope", version_name(t.binding_version), version_name(env_version)),
        );
    }

    // Envelope children: optional Header, then Body.
    let mut body_el = None;
    for c in env.children().filter(|c| c.is_element()) {
        let in_env = c.tag_name().namespace() == Some(env_ns);
        match (in_env, c.tag_name().name()) {
            (true, "Header") if body_el.is_some() => {
                r.warn(Some(tag_range(c)), "The Header must come before the Body".into())
            }
            (true, "Header") => {}
            (true, "Body") if body_el.is_some() => r.warn(Some(tag_range(c)), "The envelope has more than one Body".into()),
            (true, "Body") => body_el = Some(c),
            _ if c.tag_name().name() == "Body" || c.tag_name().name() == "Header" => r.warn(
                Some(tag_range(c)),
                format!("{} is in the wrong namespace: expected {}", label(c), ns_label(env_ns)),
            ),
            _ => r.warn(Some(tag_range(c)), format!("{} is not allowed directly inside the Envelope", label(c))),
        }
    }
    let Some(body) = body_el else {
        r.warn(Some(tag_range(env)), "The envelope has no Body".into());
        return;
    };

    // Schema documents.
    let mut docs = Vec::new();
    for text in std::iter::once(t.wsdl_text).chain(t.imports.iter().map(|i| i.text.as_str())) {
        match parse(text) {
            Ok(d) => docs.push(d),
            Err(e) => {
                r.warn(None, format!("The WSDL could not be read for validation: {e}"));
                return;
            }
        }
    }
    let idx = SchemaIndex::build(&docs);
    let mut v = Validator::new(&idx);
    let shape = &op.input_body;

    let mut kids: Vec<Node> = body.children().filter(|c| c.is_element()).collect();
    if let Some(text) = body.children().find(|c| c.is_text() && c.text().is_some_and(|s| !s.trim().is_empty())) {
        r.warn(Some(text.range()), "The Body cannot contain text, only elements".into());
    }

    // rpc: a single wrapper element named after the operation; the parts are inside it.
    let mut container = body;
    if let Some(w) = &shape.rpc_wrapper {
        let wq = (w.namespace.clone(), w.name.clone());
        match kids.first() {
            None => {
                r.warn(Some(tag_range(body)), format!("The Body is missing <{}> for operation {}", w.name, op.name));
                return;
            }
            Some(k) => {
                let q = qname_of(*k);
                if q.1 != wq.1 {
                    r.warn(Some(tag_range(*k)), format!("Expected <{}> (operation {}), found {}", w.name, op.name, label(*k)));
                    return;
                }
                if q.0 != wq.0 {
                    r.warn(
                        Some(tag_range(*k)),
                        format!("{} is in the wrong namespace: expected {}, found {}", label(*k), ns_label(&wq.0), ns_label(&q.0)),
                    );
                }
            }
        }
        for extra in &kids[1..] {
            r.warn(Some(tag_range(*extra)), format!("{} is not allowed here: an rpc Body contains only <{}>", label(*extra), w.name));
        }
        container = kids[0];
        kids = container.children().filter(|c| c.is_element()).collect();
    }

    // Parts, in order.
    let mut i = 0;
    for part in &shape.parts {
        let Some(k) = kids.get(i).copied() else {
            let name = part.element.as_ref().map_or(part.name.as_str(), |e| e.name.as_str());
            r.warn(Some(tag_range(container)), format!("{} is missing <{name}>", label(container)));
            continue;
        };
        i += 1;
        let q = qname_of(k);
        match (&part.element, &part.type_name) {
            (Some(e), _) => {
                let eq = (e.namespace.clone(), e.name.clone());
                if q.1 != eq.1 {
                    r.warn(Some(tag_range(k)), format!("Expected <{}> (operation {}), found {}", e.name, op.name, label(k)));
                    continue;
                }
                if q.0 != eq.0 {
                    r.warn(
                        Some(tag_range(k)),
                        format!("{} is in the wrong namespace: expected {}, found {}", label(k), ns_label(&eq.0), ns_label(&q.0)),
                    );
                }
                if !shape.encoded && v.has_element(&eq) {
                    v.global_element(k, &eq);
                }
            }
            (None, Some(ty)) => {
                // rpc accessor: named after the part (document style puts the type's content directly).
                if shape.rpc_wrapper.is_some() && q.1 != part.name {
                    r.warn(Some(tag_range(k)), format!("Expected <{}> (part of operation {}), found {}", part.name, op.name, label(k)));
                    continue;
                }
                if !shape.encoded {
                    v.typed_element(k, &(ty.namespace.clone(), ty.name.clone()));
                }
            }
            (None, None) => {}
        }
    }
    for extra in &kids[i.min(kids.len())..] {
        r.warn(Some(tag_range(*extra)), format!("{} is not part of operation {}", label(*extra), op.name));
    }

    for f in v.into_findings() {
        r.warn(f.range, f.message);
    }
}

//! `{{name}}` variable substitution for requests.
//!
//! Values are resolved by the caller (project variables overridden by the active
//! environment, secrets fetched from the OS keychain) and applied here just before
//! sending. Substitution into the XML body is XML-escaped; other fields are not.

use std::collections::{BTreeSet, HashMap};

use crate::http::{Auth, RequestSpec};
use crate::{Error, Result, xml_escape};

#[derive(Debug, Clone)]
pub struct VarValue {
    pub value: String,
    /// Secret values are masked wherever the sent request is shown.
    pub secret: bool,
}

pub type Vars = HashMap<String, VarValue>;

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')
}

/// Find `{{ name }}` occurrences: yields (byte range of the whole placeholder, name).
fn scan(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = text[i..].find("{{") {
        let start = i + off;
        let inner_start = start + 2;
        let Some(close) = text[inner_start..].find("}}") else { break };
        let inner = &text[inner_start..inner_start + close];
        let name = inner.trim();
        let valid = name.chars().next().is_some_and(is_name_start) && name.chars().all(is_name_char);
        if valid {
            let end = inner_start + close + 2;
            out.push((start..end, name));
            i = end;
        } else {
            i = start + 2;
        }
    }
    out
}

/// Names of all variables referenced in `text`.
pub fn references(text: &str) -> BTreeSet<String> {
    scan(text).into_iter().map(|(_, n)| n.to_string()).collect()
}

fn substitute(text: &str, vars: &Vars, escape: bool, missing: &mut BTreeSet<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (range, name) in scan(text) {
        out.push_str(&text[last..range.start]);
        match vars.get(name) {
            Some(v) if escape => out.push_str(&xml_escape(&v.value)),
            Some(v) => out.push_str(&v.value),
            None => {
                missing.insert(name.to_string());
                out.push_str(&text[range.clone()]);
            }
        }
        last = range.end;
    }
    out.push_str(&text[last..]);
    out
}

/// Apply variables to every user-editable field of a request.
/// Fails listing all undefined variables, so nothing half-substituted is ever sent.
pub fn resolve_spec(spec: &RequestSpec, vars: &Vars) -> Result<RequestSpec> {
    let mut missing = BTreeSet::new();
    let mut m = |s: &str, escape: bool| substitute(s, vars, escape, &mut missing);

    let mut out = spec.clone();
    out.endpoint = m(&spec.endpoint, false);
    out.soap_action = spec.soap_action.as_deref().map(|a| m(a, false));
    out.body = m(&spec.body, true);
    for h in &mut out.headers {
        h.name = m(&h.name, false);
        h.value = m(&h.value, false);
    }
    out.auth = match &spec.auth {
        Auth::None => Auth::None,
        Auth::Basic { username, password } => {
            Auth::Basic { username: m(username, false), password: m(password, false) }
        }
        Auth::WsSecurity(sec) => {
            let mut sec = sec.clone();
            sec.username = m(&sec.username, false);
            sec.password = m(&sec.password, false);
            Auth::WsSecurity(sec)
        }
    };

    if missing.is_empty() {
        Ok(out)
    } else {
        let list: Vec<_> = missing.into_iter().map(|n| format!("{{{{{n}}}}}")).collect();
        let (what, it) = if list.len() == 1 { ("Undefined variable", "it") } else { ("Undefined variables", "them") };
        Err(Error::Vars(format!("{what}: {}. Define {it} in Variables & Environments.", list.join(", "))))
    }
}

/// Values that must never be displayed.
pub fn secret_values(vars: &Vars) -> Vec<String> {
    vars.values().filter(|v| v.secret).map(|v| v.value.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wsdl::SoapVersion;

    fn vars(pairs: &[(&str, &str, bool)]) -> Vars {
        pairs
            .iter()
            .map(|(k, v, s)| (k.to_string(), VarValue { value: v.to_string(), secret: *s }))
            .collect()
    }

    fn spec(body: &str) -> RequestSpec {
        RequestSpec {
            endpoint: "https://{{host}}/svc".into(),
            soap_version: SoapVersion::Soap11,
            soap_action: Some("{{action}}".into()),
            body: body.into(),
            headers: vec![],
            auth: Auth::Basic { username: "{{user}}".into(), password: "{{pw}}".into() },
            timeout_secs: 5,
            accept_invalid_certs: false,
        }
    }

    #[test]
    fn scans_names_and_ignores_non_placeholders() {
        let refs = references("a {{x}} b {{ y.z }} {{1bad}} {{ }} {{a b}} {{x}} {{unterminated");
        assert_eq!(refs.into_iter().collect::<Vec<_>>(), ["x", "y.z"]);
    }

    #[test]
    fn resolves_all_fields_and_escapes_body() {
        let v = vars(&[("host", "h.example", false), ("action", "urn:a", false), ("user", "bob", false), ("pw", "p<&>", true), ("id", "C1", false)]);
        let out = resolve_spec(&spec("<id>{{id}}</id><pw>{{ pw }}</pw>"), &v).unwrap();
        assert_eq!(out.endpoint, "https://h.example/svc");
        assert_eq!(out.soap_action.as_deref(), Some("urn:a"));
        assert_eq!(out.body, "<id>C1</id><pw>p&lt;&amp;&gt;</pw>");
        match out.auth {
            Auth::Basic { username, password } => assert_eq!((username.as_str(), password.as_str()), ("bob", "p<&>")),
            _ => panic!(),
        }
        assert_eq!(secret_values(&v), ["p<&>"]);
    }

    #[test]
    fn reports_every_missing_variable() {
        let err = resolve_spec(&spec("{{id}}"), &vars(&[("user", "u", false)])).unwrap_err().to_string();
        for name in ["{{host}}", "{{action}}", "{{pw}}", "{{id}}"] {
            assert!(err.contains(name), "{err}");
        }
    }
}

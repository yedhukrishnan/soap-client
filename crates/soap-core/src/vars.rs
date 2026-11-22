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

/// Text that looks like a variable placeholder but isn't one, e.g. `{{password}` or
/// `{{ 1bad }}`: such text would be sent literally. Returns byte ranges.
pub fn malformed_placeholders(text: &str) -> Vec<std::ops::Range<usize>> {
    let valid: Vec<_> = scan(text).into_iter().map(|(r, _)| r).collect();
    let inside = |i: usize| valid.iter().any(|r| r.contains(&i));
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b' ');
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let pair = &bytes[i..i + 2];
        if (pair == b"{{" || pair == b"}}") && !inside(i) && !inside(i + 1) {
            let (mut start, mut end) = (i, i + 2);
            if pair == b"{{" {
                while end < bytes.len() && word(bytes[end]) {
                    end += 1;
                }
                let close = end;
                while end < bytes.len() && end < close + 2 && bytes[end] == b'}' {
                    end += 1;
                }
            } else {
                while start > 0 && word(bytes[start - 1]) {
                    start -= 1;
                }
                if start > 0 && bytes[start - 1] == b'{' {
                    start -= 1;
                }
            }
            // Trim the spaces `word` allowed at the edges.
            while start < end && bytes[start] == b' ' {
                start += 1;
            }
            while end > start && bytes[end - 1] == b' ' {
                end -= 1;
            }
            match out.last_mut() {
                Some(last) if start <= last.end => last.end = last.end.max(end),
                _ => out.push(start..end),
            }
            i = end.max(i + 2);
        } else {
            i += 1;
        }
    }
    out
}

/// Single-brace spellings of a defined variable, e.g. `{password}` or `{password` when
/// `password` is a variable: almost certainly a mistyped `{{password}}`. Only defined
/// names count, so legitimate braces (GUIDs like `{3F25…}`, JSON) are never matched.
/// Returns (byte range, variable name).
pub fn single_brace_placeholders<'n>(text: &str, names: &'n [String]) -> Vec<(std::ops::Range<usize>, &'n str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for (i, _) in text.match_indices('{') {
        // `{{` is a real (or separately reported broken) placeholder.
        if (i > 0 && bytes[i - 1] == b'{') || bytes.get(i + 1) == Some(&b'{') {
            continue;
        }
        let after = &text[i + 1..];
        let lead = after.len() - after.trim_start_matches(' ').len();
        let rest = &after[lead..];
        let len = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
        let word = &rest[..len];
        let Some(name) = names.iter().find(|n| n.as_str() == word) else { continue };
        let mut end = i + 1 + lead + len;
        let tail = &text[end..];
        let spaces = tail.len() - tail.trim_start_matches(' ').len();
        if tail[spaces..].starts_with('}') {
            if tail[spaces..].starts_with("}}") {
                continue; // `{name}}`: reported as a broken placeholder
            }
            end += spaces + 1;
        }
        out.push((i..end, name.as_str()));
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
    fn finds_malformed_placeholders() {
        let text = "<a>{{password}</a><b>{{ok}}</b><c>{id}}</c><d>{{ 1bad }}</d><e>{x}</e><f>{{</f>";
        let found: Vec<&str> = malformed_placeholders(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["{{password}", "{id}}", "{{ 1bad }}", "{{"]);
        assert!(malformed_placeholders("{{a}}{{ b.c }}").is_empty());
    }

    #[test]
    fn finds_single_brace_spellings_of_defined_variables() {
        let names = ["password".to_string(), "id".to_string()];
        let text = "<a>{password}</a><b>{password</b><c>{ id }</c><d>{{password}}</d><e>{other}</e><f>{3F25-AB}</f><g>{idx}</g><h>{password}}</h>";
        let found: Vec<(&str, &str)> = single_brace_placeholders(text, &names).into_iter().map(|(r, n)| (&text[r], n)).collect();
        assert_eq!(found, [("{password}", "password"), ("{password", "password"), ("{ id }", "id")]);
        assert!(single_brace_placeholders("{password}", &[]).is_empty());
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

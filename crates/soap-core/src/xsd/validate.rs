//! Checks instance XML (a request payload) against the schema components in a
//! [`SchemaIndex`]: element structure (sequence/choice/all, occurrences, namespaces),
//! attributes and simple-type values.
//!
//! The validator is deliberately forgiving: a schema construct it does not understand
//! (or a type it cannot resolve) makes it accept that part of the document rather than
//! report something that may be wrong. Values containing `{{variables}}` are not checked,
//! because their real value is only known when the request is sent.

use std::collections::HashMap;
use std::ops::Range;

use roxmltree::Node;

use super::datatypes::{
    WhiteSpace, check_builtin, compare, digits, is_numeric, length_of, normalize, quote, whitespace_of,
};
use super::{QName, SchemaIndex, element_namespace, enclosing_schema, is_xsd, resolve_qname, xsd_children};
use crate::{NS_SOAP11_ENV, NS_SOAP12_ENV, NS_XSD};

pub const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";
const MAX_DEPTH: usize = 64;
/// Stop collecting after this many findings (a wrong request can produce one per element).
const MAX_FINDINGS: usize = 200;

/// A problem found in the instance document; `range` is a byte range into its text.
#[derive(Debug, Clone)]
pub struct Finding {
    pub message: String,
    pub range: Option<Range<usize>>,
}

#[derive(Clone)]
enum Term<'a, 'i> {
    /// `name` is the qualified name instances must use; `decl` holds the type etc.
    Element { name: QName, decl: Node<'a, 'i> },
    Seq(Vec<Particle<'a, 'i>>),
    Choice(Vec<Particle<'a, 'i>>),
    All(Vec<Particle<'a, 'i>>),
    Any(Node<'a, 'i>),
    /// Something we can't model: accept whatever is left.
    Opaque,
}

#[derive(Clone)]
struct Particle<'a, 'i> {
    term: Term<'a, 'i>,
    min: u32,
    /// `None` = unbounded.
    max: Option<u32>,
}

#[derive(Clone)]
enum Simple<'a, 'i> {
    Builtin(String),
    Type(Node<'a, 'i>),
    /// A base type narrowed by the facets of an `xs:restriction` element.
    Restricted { base: Box<Simple<'a, 'i>>, by: Node<'a, 'i> },
    Any,
}

enum TypeRef<'a, 'i> {
    Simple(Simple<'a, 'i>),
    Complex(Node<'a, 'i>),
    Any,
}

#[derive(Default)]
struct Content<'a, 'i> {
    particle: Option<Particle<'a, 'i>>,
    mixed: bool,
    simple: Option<Simple<'a, 'i>>,
    /// `xs:attribute` / `xs:attributeGroup` / `xs:anyAttribute` nodes, most derived first.
    attrs: Vec<Node<'a, 'i>>,
    opaque: bool,
}

struct AttrDecl<'a, 'i> {
    name: QName,
    decl: Node<'a, 'i>,
    required: bool,
    prohibited: bool,
    fixed: Option<&'a str>,
}

#[derive(Clone, Copy)]
enum Assign<'a, 'i> {
    Decl(Node<'a, 'i>),
    Skip,
}

/// Matching state for the child elements of one instance element.
struct Matcher<'a, 'i, 'd, 'x> {
    kids: Vec<Node<'d, 'x>>,
    pos: usize,
    assigned: Vec<Option<Assign<'a, 'i>>>,
    /// Already reported (out of place): skipped by matching and by the leftover pass.
    reported: Vec<bool>,
    /// Every element name the content model can contain.
    names: Vec<QName>,
    opaque: bool,
}

impl<'a, 'i, 'd, 'x> Matcher<'a, 'i, 'd, 'x> {
    fn current(&mut self) -> Option<Node<'d, 'x>> {
        while self.pos < self.kids.len() && self.reported[self.pos] {
            self.pos += 1;
        }
        self.kids.get(self.pos).copied()
    }

    fn take(&mut self, a: Assign<'a, 'i>) {
        self.assigned[self.pos] = Some(a);
        self.pos += 1;
    }
}

pub struct Validator<'s, 'a, 'i> {
    idx: &'s SchemaIndex<'a, 'i>,
    /// Substitution group head -> member element names.
    subst: HashMap<QName, Vec<QName>>,
    findings: Vec<Finding>,
}

fn qname_of(n: Node) -> QName {
    (n.tag_name().namespace().unwrap_or("").to_string(), n.tag_name().name().to_string())
}

fn occurs(n: Node) -> (u32, Option<u32>) {
    let min = n.attribute("minOccurs").and_then(|v| v.trim().parse().ok()).unwrap_or(1);
    let max = match n.attribute("maxOccurs").map(str::trim) {
        Some("unbounded") => None,
        Some(v) => Some(v.parse().unwrap_or(1)),
        None => Some(1),
    };
    (min, max)
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v.map(str::trim), Some("true" | "1"))
}

/// Values with `{{variables}}` aren't checked: their real value is only known when sending.
/// (Broken placeholders such as `{{name}` are reported separately.)
fn has_placeholder(v: &str) -> bool {
    v.contains("{{") || v.contains("}}")
}

fn ns_label(ns: &str) -> String {
    if ns.is_empty() { "no namespace".into() } else { format!("\"{ns}\"") }
}

/// Byte range of an element's start-tag name, including the `<`.
fn tag_range(n: Node) -> Range<usize> {
    let text = n.document().input_text();
    let start = n.range().start;
    let rest = &text[start + 1..];
    let len = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len());
    start..start + 1 + len
}

fn text_of(n: Node) -> String {
    n.children().filter(|c| c.is_text()).filter_map(|c| c.text()).collect()
}

/// Range of an element's text content (for value errors), or its tag if empty.
fn value_range(n: Node) -> Range<usize> {
    n.children()
        .find(|c| c.is_text() && c.text().is_some_and(|t| !t.trim().is_empty()))
        .map(|t| {
            let r = t.range();
            let s = t.text().unwrap_or("");
            let lead = s.len() - s.trim_start().len();
            let trail = s.len() - s.trim_end().len();
            // Text node ranges cover the raw source; trim only when it matches the text.
            if r.len() == s.len() { r.start + lead..r.end - trail } else { r }
        })
        .unwrap_or_else(|| tag_range(n))
}

fn label(n: Node) -> String {
    format!("<{}>", n.tag_name().name())
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != *cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn suggest<'v>(v: &str, options: &[&'v str]) -> Option<&'v str> {
    if let Some(o) = options.iter().find(|o| o.eq_ignore_ascii_case(v)) {
        return Some(o);
    }
    let limit = (v.chars().count() / 4).max(1);
    options
        .iter()
        .map(|o| (levenshtein(&v.to_lowercase(), &o.to_lowercase()), *o))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, o)| o)
}

impl<'s, 'a, 'i> Validator<'s, 'a, 'i> {
    pub fn new(idx: &'s SchemaIndex<'a, 'i>) -> Self {
        let mut subst: HashMap<QName, Vec<QName>> = HashMap::new();
        for (qn, el) in &idx.elements {
            if let Some(head) = el.attribute("substitutionGroup") {
                // XSD 1.1 allows a list; take each.
                for h in head.split_whitespace() {
                    subst.entry(resolve_qname(*el, h)).or_default().push(qn.clone());
                }
            }
        }
        for members in subst.values_mut() {
            members.sort();
        }
        Self { idx, subst, findings: Vec::new() }
    }

    pub fn into_findings(self) -> Vec<Finding> {
        self.findings
    }

    fn report(&mut self, range: Option<Range<usize>>, message: String) {
        if self.findings.len() < MAX_FINDINGS {
            self.findings.push(Finding { message, range });
        }
    }

    /// Whether a global element with this name is declared.
    pub fn has_element(&self, qn: &QName) -> bool {
        self.idx.elements.contains_key(qn)
    }

    /// Validate an instance element that should be the global element `qn`
    /// (its name has already been checked by the caller).
    pub fn global_element(&mut self, inst: Node, qn: &QName) {
        if let Some(decl) = self.idx.elements.get(qn).copied() {
            self.element(inst, decl, 0);
        }
    }

    /// Validate an instance element whose content is defined by the type `type_qn`
    /// (rpc-style `type=` parts).
    pub fn typed_element(&mut self, inst: Node, type_qn: &QName) {
        if self.nil(inst) == Some(true) {
            if inst.children().any(|c| c.is_element()) || !text_of(inst).trim().is_empty() {
                self.report(Some(tag_range(inst)), format!("{} is nil, so it must be empty", label(inst)));
            }
            return;
        }
        let ty = self.type_by_name(type_qn);
        self.check_type(inst, ty, None, 0);
    }

    // ---- schema lookups ----

    fn type_by_name(&self, qn: &QName) -> TypeRef<'a, 'i> {
        if qn.0 == NS_XSD {
            return match qn.1.as_str() {
                "anyType" => TypeRef::Any,
                "anySimpleType" => TypeRef::Simple(Simple::Any),
                b => TypeRef::Simple(Simple::Builtin(b.to_string())),
            };
        }
        if let Some(ct) = self.idx.complex_types.get(qn) {
            return TypeRef::Complex(*ct);
        }
        if let Some(st) = self.idx.simple_types.get(qn) {
            return TypeRef::Simple(Simple::Type(*st));
        }
        TypeRef::Any
    }

    fn decl_type(&self, decl: Node<'a, 'i>, depth: usize) -> TypeRef<'a, 'i> {
        if let Some(t) = decl.attribute("type") {
            return self.type_by_name(&resolve_qname(decl, t));
        }
        if let Some(ct) = xsd_children(decl).find(|c| is_xsd(c, "complexType")) {
            return TypeRef::Complex(ct);
        }
        if let Some(st) = xsd_children(decl).find(|c| is_xsd(c, "simpleType")) {
            return TypeRef::Simple(Simple::Type(st));
        }
        // A substitution group member without a type takes the head's type.
        if depth < 8
            && let Some(h) = decl.attribute("substitutionGroup").and_then(|h| h.split_whitespace().next())
            && let Some(head) = self.idx.elements.get(&resolve_qname(decl, h))
        {
            return self.decl_type(*head, depth + 1);
        }
        TypeRef::Any
    }

    fn simple_by_name(&self, qn: &QName) -> Simple<'a, 'i> {
        match self.type_by_name(qn) {
            TypeRef::Simple(s) => s,
            _ => Simple::Any,
        }
    }

    // ---- content models ----

    fn particle(&self, p: Node<'a, 'i>, depth: usize) -> Option<Particle<'a, 'i>> {
        if depth > MAX_DEPTH {
            return Some(Particle { term: Term::Opaque, min: 0, max: None });
        }
        let (min, max) = occurs(p);
        let group = |items: Node<'a, 'i>| -> Vec<Particle<'a, 'i>> {
            xsd_children(items).filter_map(|c| self.particle(c, depth + 1)).collect()
        };
        let term = match p.tag_name().name() {
            "element" => {
                if let Some(r) = p.attribute("ref") {
                    let qn = resolve_qname(p, r);
                    match self.idx.elements.get(&qn) {
                        Some(decl) => Term::Element { name: qn, decl: *decl },
                        None => Term::Opaque,
                    }
                } else {
                    let name = p.attribute("name")?;
                    Term::Element { name: (element_namespace(p).unwrap_or_default(), name.to_string()), decl: p }
                }
            }
            "sequence" => Term::Seq(group(p)),
            "choice" => Term::Choice(group(p)),
            "all" => Term::All(group(p)),
            "any" => Term::Any(p),
            "group" => {
                let g = p.attribute("ref").and_then(|r| self.idx.groups.get(&resolve_qname(p, r)).copied());
                let model = g.and_then(|g| {
                    xsd_children(g).find(|c| matches!(c.tag_name().name(), "sequence" | "choice" | "all"))
                });
                match model {
                    Some(m) => self.particle(m, depth + 1)?.term,
                    None => Term::Opaque,
                }
            }
            _ => return None,
        };
        Some(Particle { term, min, max })
    }

    fn content(&self, ct: Node<'a, 'i>, depth: usize) -> Content<'a, 'i> {
        let mut c = Content { mixed: truthy(ct.attribute("mixed")), ..Default::default() };
        if depth > 16 {
            c.opaque = true;
            return c;
        }
        let attr_like = |n: &Node| matches!(n.tag_name().name(), "attribute" | "attributeGroup" | "anyAttribute");
        for child in xsd_children(ct) {
            match child.tag_name().name() {
                "sequence" | "choice" | "all" | "group" => c.particle = self.particle(child, 0),
                "attribute" | "attributeGroup" | "anyAttribute" => c.attrs.push(child),
                "simpleContent" | "complexContent" => {
                    let simple_content = child.tag_name().name() == "simpleContent";
                    c.mixed |= truthy(child.attribute("mixed"));
                    let Some(der) = xsd_children(child).next() else { continue };
                    let extension = der.tag_name().name() == "extension";
                    let base_qn = der.attribute("base").map(|b| resolve_qname(der, b));
                    let base = base_qn.as_ref().map(|q| self.type_by_name(q));
                    c.attrs.extend(xsd_children(der).filter(attr_like));
                    let base_content = match &base {
                        Some(TypeRef::Complex(b)) => Some(self.content(*b, depth + 1)),
                        _ => None,
                    };
                    if let Some(bc) = &base_content {
                        c.attrs.extend(bc.attrs.iter().copied());
                        c.opaque |= bc.opaque;
                    }
                    if simple_content {
                        let base_simple = match (base, &base_content) {
                            (Some(TypeRef::Simple(s)), _) => s,
                            (_, Some(bc)) => bc.simple.clone().unwrap_or(Simple::Any),
                            _ => Simple::Any,
                        };
                        c.simple = Some(if extension {
                            base_simple
                        } else {
                            Simple::Restricted { base: Box::new(base_simple), by: der }
                        });
                    } else {
                        let own = xsd_children(der).find(|n| !attr_like(n)).and_then(|n| self.particle(n, 0));
                        let is_any_type = base_qn.as_ref().is_some_and(|q| q.0 == NS_XSD && q.1 == "anyType");
                        if base_content.is_none() && !is_any_type {
                            c.opaque = true; // base type not found
                        }
                        c.particle = if extension {
                            match (base_content.and_then(|b| b.particle), own) {
                                (Some(b), Some(o)) => Some(Particle { term: Term::Seq(vec![b, o]), min: 1, max: Some(1) }),
                                (b, o) => b.or(o),
                            }
                        } else {
                            own
                        };
                    }
                }
                _ => {}
            }
        }
        c
    }

    fn attr_decls(&self, nodes: &[Node<'a, 'i>], out: &mut Vec<AttrDecl<'a, 'i>>, any: &mut bool, depth: usize) {
        if depth > 10 {
            *any = true;
            return;
        }
        for n in nodes {
            match n.tag_name().name() {
                "anyAttribute" => *any = true,
                "attributeGroup" => {
                    let g = n.attribute("ref").and_then(|r| self.idx.attribute_groups.get(&resolve_qname(*n, r)).copied());
                    match g {
                        Some(g) => self.attr_decls(&xsd_children(g).collect::<Vec<_>>(), out, any, depth + 1),
                        None => *any = true,
                    }
                }
                "attribute" => {
                    let (name, decl) = if let Some(r) = n.attribute("ref") {
                        let qn = resolve_qname(*n, r);
                        match self.idx.attributes.get(&qn) {
                            Some(d) => (qn, *d),
                            None => (qn, *n),
                        }
                    } else if let Some(local) = n.attribute("name") {
                        let schema = enclosing_schema(*n);
                        let qualified = n.attribute("form").or_else(|| schema.and_then(|s| s.attribute("attributeFormDefault")))
                            == Some("qualified");
                        let ns = if qualified {
                            schema.and_then(|s| s.attribute("targetNamespace")).unwrap_or("").to_string()
                        } else {
                            String::new()
                        };
                        ((ns, local.to_string()), *n)
                    } else {
                        continue;
                    };
                    if out.iter().any(|d| d.name == name) {
                        continue; // a more derived declaration wins
                    }
                    let usage = n.attribute("use").map(str::trim);
                    out.push(AttrDecl {
                        name,
                        decl,
                        required: usage == Some("required"),
                        prohibited: usage == Some("prohibited"),
                        fixed: n.attribute("fixed").or_else(|| decl.attribute("fixed")),
                    });
                }
                _ => {}
            }
        }
    }

    fn attr_simple(&self, decl: Node<'a, 'i>) -> Simple<'a, 'i> {
        if let Some(t) = decl.attribute("type") {
            return self.simple_by_name(&resolve_qname(decl, t));
        }
        match xsd_children(decl).find(|c| is_xsd(c, "simpleType")) {
            Some(st) => Simple::Type(st),
            None => Simple::Any,
        }
    }

    // ---- simple values ----

    /// The built-in type a simple type is ultimately derived from (`#list` for lists).
    fn builtin_of(&self, s: &Simple<'a, 'i>, depth: usize) -> Option<String> {
        if depth > 32 {
            return None;
        }
        match s {
            Simple::Builtin(b) => Some(b.clone()),
            Simple::Any => None,
            Simple::Restricted { base, .. } => self.builtin_of(base, depth + 1),
            Simple::Type(st) => {
                let d = xsd_children(*st).next()?;
                match d.tag_name().name() {
                    "restriction" => self.builtin_of(&self.restriction_base(d), depth + 1),
                    "list" => Some("#list".into()),
                    _ => None,
                }
            }
        }
    }

    fn restriction_base(&self, r: Node<'a, 'i>) -> Simple<'a, 'i> {
        if let Some(b) = r.attribute("base") {
            return self.simple_by_name(&resolve_qname(r, b));
        }
        match xsd_children(r).find(|c| is_xsd(c, "simpleType")) {
            Some(st) => Simple::Type(st),
            None => Simple::Any,
        }
    }

    fn whitespace(&self, s: &Simple<'a, 'i>) -> WhiteSpace {
        match self.builtin_of(s, 0) {
            Some(b) if b != "#list" => whitespace_of(&b),
            _ => WhiteSpace::Collapse,
        }
    }

    /// Check a raw value (whitespace is normalised here).
    fn check_simple(&self, s: &Simple<'a, 'i>, raw: &str) -> Result<(), String> {
        let v = normalize(raw, self.whitespace(s));
        self.check_normalized(s, &v, 0)
    }

    fn check_normalized(&self, s: &Simple<'a, 'i>, v: &str, depth: usize) -> Result<(), String> {
        if depth > 32 {
            return Ok(());
        }
        match s {
            Simple::Any => Ok(()),
            Simple::Builtin(b) => check_builtin(b, v),
            Simple::Restricted { base, by } => {
                self.check_normalized(base, v, depth + 1)?;
                self.facets(*by, v, self.builtin_of(base, 0).as_deref())
            }
            Simple::Type(st) => {
                let Some(d) = xsd_children(*st).next() else { return Ok(()) };
                match d.tag_name().name() {
                    "restriction" => {
                        let base = self.restriction_base(d);
                        self.check_normalized(&Simple::Restricted { base: Box::new(base), by: d }, v, depth + 1)
                    }
                    "list" => {
                        let item = match d.attribute("itemType") {
                            Some(t) => self.simple_by_name(&resolve_qname(d, t)),
                            None => xsd_children(d).find(|c| is_xsd(c, "simpleType")).map(Simple::Type).unwrap_or(Simple::Any),
                        };
                        for it in v.split(' ').filter(|s| !s.is_empty()) {
                            self.check_normalized(&item, it, depth + 1)?;
                        }
                        Ok(())
                    }
                    "union" => {
                        let mut members: Vec<Simple> = d
                            .attribute("memberTypes")
                            .map(|m| m.split_whitespace().map(|t| self.simple_by_name(&resolve_qname(d, t))).collect())
                            .unwrap_or_default();
                        members.extend(xsd_children(d).filter(|c| is_xsd(c, "simpleType")).map(Simple::Type));
                        if members.is_empty() || members.iter().any(|m| self.check_simple(m, v).is_ok()) {
                            Ok(())
                        } else {
                            Err(format!("{} does not match any of the allowed types", quote(v)))
                        }
                    }
                    _ => Ok(()),
                }
            }
        }
    }

    fn facets(&self, by: Node<'a, 'i>, v: &str, builtin: Option<&str>) -> Result<(), String> {
        let b = builtin.unwrap_or("string");
        let facet = |name: &str| xsd_children(by).find(|c| c.tag_name().name() == name).and_then(|f| f.attribute("value"));
        let num = |name: &str| facet(name).and_then(|f| f.trim().parse::<usize>().ok());

        let enums: Vec<&str> = xsd_children(by).filter(|c| is_xsd(c, "enumeration")).filter_map(|e| e.attribute("value")).collect();
        if !enums.is_empty() {
            let ws = builtin.filter(|b| *b != "#list").map(whitespace_of).unwrap_or(WhiteSpace::Collapse);
            let matches = enums.iter().any(|e| {
                let e = normalize(e, ws);
                e == v || compare(b, &e, v) == Some(std::cmp::Ordering::Equal)
            });
            if !matches {
                let mut msg = format!("{} is not an allowed value", quote(v));
                if let Some(s) = suggest(v, &enums) {
                    msg.push_str(&format!(". Did you mean \"{s}\"?"));
                } else {
                    const SHOW: usize = 8;
                    let shown: Vec<String> = enums.iter().take(SHOW).map(|e| format!("\"{e}\"")).collect();
                    let more = if enums.len() > SHOW { format!(" … ({} more)", enums.len() - SHOW) } else { String::new() };
                    msg.push_str(&format!(". Allowed: {}{more}", shown.join(", ")));
                }
                return Err(msg);
            }
        }

        let (len, unit) = if b == "#list" {
            (Some(v.split(' ').filter(|s| !s.is_empty()).count()), "items")
        } else if matches!(b, "hexBinary" | "base64Binary") {
            (length_of(b, v), "bytes")
        } else {
            (length_of(b, v), "characters")
        };
        if let Some(n) = len {
            let unit = |k: usize| if k == 1 { unit.trim_end_matches('s') } else { unit };
            if let Some(l) = num("length").filter(|l| n != *l) {
                return Err(format!("{} must be exactly {l} {} long (it has {n})", quote(v), unit(l)));
            }
            if let Some(l) = num("minLength").filter(|l| n < *l) {
                return Err(if n == 0 { "the value cannot be empty".into() } else { format!("{} is too short: {n} {}, at least {l} required", quote(v), unit(n)) });
            }
            if let Some(l) = num("maxLength").filter(|l| n > *l) {
                return Err(format!("{} is too long: {n} {}, at most {l} allowed", quote(v), unit(n)));
            }
        }

        if is_numeric(b) {
            use std::cmp::Ordering::*;
            let bound = |name: &str, bad: &[std::cmp::Ordering], what: &str| -> Result<(), String> {
                match facet(name) {
                    Some(f) if compare(b, v, f.trim()).is_some_and(|o| bad.contains(&o)) => {
                        Err(format!("{} must be {what} {}", quote(v), f.trim()))
                    }
                    _ => Ok(()),
                }
            };
            bound("minInclusive", &[Less], "at least")?;
            bound("maxInclusive", &[Greater], "at most")?;
            bound("minExclusive", &[Less, Equal], "greater than")?;
            bound("maxExclusive", &[Greater, Equal], "less than")?;
            if let Some((total, frac)) = digits(v) {
                if let Some(t) = num("totalDigits").filter(|t| total > *t) {
                    return Err(format!("{} has too many digits (at most {t})", quote(v)));
                }
                if let Some(f) = num("fractionDigits").filter(|f| frac > *f) {
                    return Err(format!("{} has too many decimal places (at most {f})", quote(v)));
                }
            }
        }
        // `pattern` (XSD regular expressions) is not checked.
        Ok(())
    }

    /// Report a value problem, skipping templated values and flagging `?` placeholders.
    fn check_value(&mut self, what: &str, raw: &str, s: &Simple<'a, 'i>, fixed: Option<&str>, range: Range<usize>) {
        if has_placeholder(raw) {
            return;
        }
        if raw.trim() == "?" {
            self.report(Some(range), format!("{what} still has the placeholder value \"?\""));
            return;
        }
        if let Err(e) = self.check_simple(s, raw) {
            self.report(Some(range), format!("{what}: {e}"));
            return;
        }
        if let Some(f) = fixed {
            let ws = self.whitespace(s);
            if normalize(raw, ws) != normalize(f, ws) {
                self.report(Some(range), format!("{what} must be \"{f}\" (fixed by the schema)"));
            }
        }
    }

    // ---- elements ----

    /// `Some(true)` if `xsi:nil="true"`, `Some(false)` for an explicit false, `None` if absent.
    fn nil(&self, inst: Node) -> Option<bool> {
        inst.attribute((NS_XSI, "nil")).map(|v| truthy(Some(v)))
    }

    fn element(&mut self, inst: Node, decl: Node<'a, 'i>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        if self.nil(inst) == Some(true) {
            if !truthy(decl.attribute("nillable")) {
                self.report(Some(tag_range(inst)), format!("{} cannot be nil: the schema does not mark it nillable", label(inst)));
            } else if inst.children().any(|c| c.is_element()) || !text_of(inst).trim().is_empty() {
                self.report(Some(tag_range(inst)), format!("{} is nil, so it must be empty", label(inst)));
            }
            return;
        }
        let ty = match inst.attribute((NS_XSI, "type")) {
            Some(t) => self.type_by_name(&resolve_qname(inst, t)),
            None => self.decl_type(decl, 0),
        };
        self.check_type(inst, ty, decl.attribute("fixed"), depth);
    }

    fn check_type(&mut self, inst: Node, ty: TypeRef<'a, 'i>, fixed: Option<&str>, depth: usize) {
        let kids: Vec<Node> = inst.children().filter(|c| c.is_element()).collect();
        match ty {
            TypeRef::Any => {}
            TypeRef::Simple(s) => {
                self.check_attributes(inst, &[], false);
                if let Some(k) = kids.first() {
                    self.report(Some(tag_range(*k)), format!("{} must contain only a value, not child elements", label(inst)));
                    return;
                }
                self.check_value(&label(inst), &text_of(inst), &s, fixed, value_range(inst));
            }
            TypeRef::Complex(ct) => {
                let content = self.content(ct, 0);
                if content.opaque {
                    return;
                }
                self.check_attributes(inst, &content.attrs, true);
                if let Some(s) = &content.simple {
                    if let Some(k) = kids.first() {
                        self.report(Some(tag_range(*k)), format!("{} must contain only a value, not child elements", label(inst)));
                        return;
                    }
                    self.check_value(&label(inst), &text_of(inst), s, fixed, value_range(inst));
                    return;
                }
                if !content.mixed
                    && let Some(t) = inst.children().find(|c| c.is_text() && c.text().is_some_and(|t| !t.trim().is_empty()))
                {
                    let msg = if t.text().unwrap_or("").trim() == "?" {
                        format!("{} still has the placeholder \"?\"; it should contain child elements", label(inst))
                    } else {
                        format!("{} cannot contain text, only child elements", label(inst))
                    };
                    self.report(Some(value_range(inst)), msg);
                }
                match content.particle {
                    None => {
                        for k in &kids {
                            self.report(Some(tag_range(*k)), format!("{} is not allowed: {} must be empty", label(*k), label(inst)));
                        }
                    }
                    Some(p) => self.children(inst, kids, &p, depth),
                }
            }
        }
    }

    fn children<'d, 'x>(&mut self, parent: Node<'d, 'x>, kids: Vec<Node<'d, 'x>>, p: &Particle<'a, 'i>, depth: usize) {
        let n = kids.len();
        let mut names = Vec::new();
        self.collect_names(p, &mut names, &mut false, 0);
        let mut m = Matcher {
            kids,
            pos: 0,
            assigned: vec![None; n],
            reported: vec![false; n],
            names,
            opaque: false,
        };
        self.run(p, &mut m, parent, &[]);
        if !m.opaque {
            for i in 0..n {
                if m.assigned[i].is_none() && !m.reported[i] {
                    let k = m.kids[i];
                    self.unexpected(k, &m, parent);
                }
            }
        }
        for i in 0..n {
            if let Some(Assign::Decl(d)) = m.assigned[i] {
                self.element(m.kids[i], d, depth + 1);
            }
        }
    }

    fn collect_names(&self, p: &Particle<'a, 'i>, out: &mut Vec<QName>, wildcard: &mut bool, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match &p.term {
            Term::Element { name, .. } => self.with_members(name, out, 0),
            Term::Seq(items) | Term::Choice(items) | Term::All(items) => {
                for it in items {
                    self.collect_names(it, out, wildcard, depth + 1);
                }
            }
            Term::Any(_) | Term::Opaque => *wildcard = true,
        }
    }

    fn with_members(&self, name: &QName, out: &mut Vec<QName>, depth: usize) {
        if depth > 8 || out.contains(name) {
            return;
        }
        out.push(name.clone());
        for m in self.subst.get(name).into_iter().flatten() {
            self.with_members(m, out, depth + 1);
        }
    }

    /// The declaration an instance element named `kid` uses when it fills an element
    /// particle (directly, or as a substitution group member).
    fn element_matches(&self, name: &QName, decl: Node<'a, 'i>, kid: &QName, depth: usize) -> Option<Node<'a, 'i>> {
        if kid == name && !truthy(decl.attribute("abstract")) {
            return Some(decl);
        }
        if depth > 8 {
            return None;
        }
        for m in self.subst.get(name).into_iter().flatten() {
            if let Some(md) = self.idx.elements.get(m)
                && let Some(d) = self.element_matches(m, *md, kid, depth + 1)
            {
                return Some(d);
            }
        }
        None
    }

    fn any_allows(&self, any: Node<'a, 'i>, ns: &str) -> bool {
        let tns = enclosing_schema(any).and_then(|s| s.attribute("targetNamespace")).unwrap_or("");
        let spec = any.attribute("namespace").unwrap_or("##any");
        spec.split_whitespace().any(|t| match t {
            "##any" => true,
            "##other" => ns != tns && !ns.is_empty(),
            "##targetNamespace" => ns == tns,
            "##local" => ns.is_empty(),
            uri => ns == uri,
        })
    }

    /// Whether `kid` can be the first element matched by `t`. `loose` also accepts an
    /// element with the right local name in the wrong namespace (reported separately).
    fn can_start(&self, t: &Term<'a, 'i>, kid: Node, loose: bool, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        match t {
            Term::Element { name, decl } => {
                let q = qname_of(kid);
                self.element_matches(name, *decl, &q, 0).is_some() || (loose && q.1 == name.1)
            }
            Term::Any(a) => self.any_allows(*a, kid.tag_name().namespace().unwrap_or("")),
            Term::Opaque => true,
            Term::Choice(items) | Term::All(items) => items.iter().any(|i| self.can_start(&i.term, kid, loose, depth + 1)),
            Term::Seq(items) => {
                for i in items {
                    if self.can_start(&i.term, kid, loose, depth + 1) {
                        return true;
                    }
                    if !self.nullable(i, depth + 1) {
                        return false;
                    }
                }
                false
            }
        }
    }

    fn nullable(&self, p: &Particle<'a, 'i>, depth: usize) -> bool {
        p.min == 0 || self.nullable_term(&p.term, depth)
    }

    fn nullable_term(&self, t: &Term<'a, 'i>, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return true;
        }
        match t {
            Term::Element { .. } | Term::Any(_) => false,
            Term::Opaque => true,
            Term::Seq(items) | Term::All(items) => items.iter().all(|i| self.nullable(i, depth + 1)),
            Term::Choice(items) => items.is_empty() || items.iter().any(|i| self.nullable(i, depth + 1)),
        }
    }

    /// Match particle `p` against the children at the current position, reporting what
    /// doesn't fit. `follow` are the particles that may come after it (for recovery).
    fn run<'d, 'x>(
        &mut self,
        p: &Particle<'a, 'i>,
        m: &mut Matcher<'a, 'i, 'd, 'x>,
        parent: Node<'d, 'x>,
        follow: &[&Particle<'a, 'i>],
    ) {
        if m.opaque {
            return;
        }
        let below_max = |count: u32| p.max.is_none_or(|mx| count < mx);
        match &p.term {
            Term::Opaque => {
                m.opaque = true;
                m.pos = m.kids.len();
            }
            Term::Element { name, decl } => {
                let mut count = 0;
                while below_max(count) {
                    let Some(kid) = m.current() else { break };
                    let q = qname_of(kid);
                    match self.element_matches(name, *decl, &q, 0) {
                        Some(d) => m.take(Assign::Decl(d)),
                        None if q == *name => {
                            let mut members = Vec::new();
                            self.with_members(name, &mut members, 0);
                            let list: Vec<String> = members.iter().skip(1).map(|n| format!("<{}>", n.1)).collect();
                            self.report(
                                Some(tag_range(kid)),
                                format!("{} is abstract: use one of {} instead", label(kid), list.join(", ")),
                            );
                            m.take(Assign::Skip);
                        }
                        // Right name, wrong namespace: say so, and still check its content.
                        None if q.1 == name.1 && q.0 != name.0 => {
                            self.report(
                                Some(tag_range(kid)),
                                format!("{} is in the wrong namespace: expected {}, found {}", label(kid), ns_label(&name.0), ns_label(&q.0)),
                            );
                            m.take(Assign::Decl(*decl));
                        }
                        None => break,
                    }
                    count += 1;
                }
                if count < p.min {
                    self.missing(name, *decl, m, parent, p.min, count);
                }
            }
            Term::Any(a) => {
                let mut count = 0;
                while below_max(count) {
                    let Some(kid) = m.current() else { break };
                    if !self.any_allows(*a, kid.tag_name().namespace().unwrap_or("")) {
                        break;
                    }
                    let lax = (a.attribute("processContents") != Some("skip"))
                        .then(|| self.idx.elements.get(&qname_of(kid)).copied())
                        .flatten();
                    m.take(lax.map(Assign::Decl).unwrap_or(Assign::Skip));
                    count += 1;
                }
                if count < p.min {
                    self.report(Some(tag_range(parent)), format!("{} needs at least {} more element(s) at this point", label(parent), p.min - count));
                }
            }
            _ => {
                if p.max == Some(1) {
                    let starts = m.current().is_some_and(|k| self.can_start(&p.term, k, true, 0));
                    if p.min == 0 && !starts {
                        return;
                    }
                    self.run_once(&p.term, m, parent, follow);
                } else {
                    // The group may repeat, so it can follow itself.
                    let again: Vec<&Particle> = std::iter::once(p).chain(follow.iter().copied()).collect();
                    let mut count = 0;
                    while below_max(count) {
                        let Some(kid) = m.current() else { break };
                        if !self.can_start(&p.term, kid, true, 0) {
                            break;
                        }
                        let before = m.pos;
                        self.run_once(&p.term, m, parent, &again);
                        count += 1;
                        if m.pos == before {
                            break;
                        }
                    }
                    if count == 0 && p.min > 0 && !self.nullable_term(&p.term, 0) {
                        self.run_once(&p.term, m, parent, follow); // reports what is missing
                    }
                }
            }
        }
    }

    fn run_once<'d, 'x>(
        &mut self,
        t: &Term<'a, 'i>,
        m: &mut Matcher<'a, 'i, 'd, 'x>,
        parent: Node<'d, 'x>,
        follow: &[&Particle<'a, 'i>],
    ) {
        match t {
            Term::Seq(items) => {
                for i in 0..items.len() {
                    let rest: Vec<&Particle> = items[i..].iter().chain(follow.iter().copied()).collect();
                    self.skip_unmatchable(m, parent, &rest);
                    self.run(&items[i], m, parent, &rest[1..]);
                }
            }
            Term::Choice(items) => {
                let candidates: Vec<&Particle> = items.iter().chain(follow.iter().copied()).collect();
                self.skip_unmatchable(m, parent, &candidates);
                let kid = m.current();
                let pick = kid.and_then(|k| {
                    items
                        .iter()
                        .find(|i| self.can_start(&i.term, k, false, 0))
                        .or_else(|| items.iter().find(|i| self.can_start(&i.term, k, true, 0)))
                });
                match pick {
                    Some(it) => self.run(it, m, parent, follow),
                    None if items.iter().any(|i| self.nullable(i, 0)) => {}
                    None => {
                        let mut names = Vec::new();
                        let mut wild = false;
                        for i in items {
                            self.collect_names(i, &mut names, &mut wild, 0);
                        }
                        let list: Vec<String> = names.iter().map(|n| format!("<{}>", n.1)).collect();
                        let what = format!("one of {}", list.join(", "));
                        match kid {
                            Some(k) => self.report(Some(tag_range(k)), format!("Expected {what} before {}", label(k))),
                            None => self.report(Some(tag_range(parent)), format!("{} is missing {what}", label(parent))),
                        }
                    }
                }
            }
            Term::All(items) => {
                let mut used = vec![false; items.len()];
                let candidates: Vec<&Particle> = items.iter().chain(follow.iter().copied()).collect();
                loop {
                    self.skip_unmatchable(m, parent, &candidates);
                    let Some(kid) = m.current() else { break };
                    let Some(i) = (0..items.len()).find(|i| !used[*i] && self.can_start(&items[*i].term, kid, true, 0)) else {
                        break;
                    };
                    used[i] = true;
                    self.run(&items[i], m, parent, &candidates);
                }
                for (i, it) in items.iter().enumerate() {
                    if !used[i] && !self.nullable(it, 0) {
                        self.run(it, m, parent, follow);
                    }
                }
            }
            other => {
                let p = Particle { term: other.clone(), min: 1, max: Some(1) };
                self.run(&p, m, parent, follow);
            }
        }
    }

    /// Skip (and report) children that none of `candidates` can match, e.g. an unknown
    /// element or one too many, so that a single stray element doesn't make everything
    /// after it look missing.
    fn skip_unmatchable<'d, 'x>(
        &mut self,
        m: &mut Matcher<'a, 'i, 'd, 'x>,
        parent: Node<'d, 'x>,
        candidates: &[&Particle<'a, 'i>],
    ) {
        while let Some(k) = m.current() {
            if candidates.iter().any(|c| self.can_start(&c.term, k, true, 0)) {
                break;
            }
            self.unexpected(k, m, parent);
            m.reported[m.pos] = true;
            m.assigned[m.pos] = Some(Assign::Skip);
        }
    }

    fn unexpected<'d, 'x>(&mut self, k: Node<'d, 'x>, m: &Matcher<'a, 'i, 'd, 'x>, parent: Node<'d, 'x>) {
        let q = qname_of(k);
        let msg = if let Some(n) = m.names.iter().find(|n| n.1 == q.1 && n.0 != q.0) {
            format!("{} is in the wrong namespace: expected {}, found {}", label(k), ns_label(&n.0), ns_label(&q.0))
        } else if m.names.contains(&q) {
            format!("{} is not allowed here: it appears too many times or in the wrong position", label(k))
        } else {
            format!("{} is not allowed in {}", label(k), label(parent))
        };
        self.report(Some(tag_range(k)), msg);
    }

    fn missing<'d, 'x>(
        &mut self,
        name: &QName,
        decl: Node<'a, 'i>,
        m: &mut Matcher<'a, 'i, 'd, 'x>,
        parent: Node<'d, 'x>,
        min: u32,
        count: u32,
    ) {
        let kid = m.current();
        // Present, but later than it should be.
        if let Some(k) = kid {
            let later = (m.pos + 1..m.kids.len()).find_map(|j| {
                (!m.reported[j] && m.assigned[j].is_none())
                    .then(|| self.element_matches(name, decl, &qname_of(m.kids[j]), 0).map(|d| (j, d)))
                    .flatten()
            });
            if let Some((j, d)) = later {
                self.report(
                    Some(tag_range(m.kids[j])),
                    format!("{} is in the wrong position: it must come before {}", label(m.kids[j]), label(k)),
                );
                m.assigned[j] = Some(Assign::Decl(d));
                m.reported[j] = true;
                return;
            }
        }
        let what = if min > 1 {
            format!("at least {min} <{}> elements (found {count})", name.1)
        } else {
            format!("required element <{}>", name.1)
        };
        match kid {
            Some(k) => self.report(Some(tag_range(k)), format!("Missing {what} before {}", label(k))),
            None => self.report(Some(tag_range(parent)), format!("{} is missing {what}", label(parent))),
        }
    }

    fn check_attributes(&mut self, inst: Node, attr_nodes: &[Node<'a, 'i>], complex: bool) {
        let mut decls = Vec::new();
        let mut any = false;
        self.attr_decls(attr_nodes, &mut decls, &mut any, 0);
        if complex {
            for d in decls.iter().filter(|d| d.required && !d.prohibited) {
                let present = inst.attributes().any(|a| a.namespace().unwrap_or("") == d.name.0 && a.name() == d.name.1);
                if !present {
                    self.report(Some(tag_range(inst)), format!("{} is missing required attribute \"{}\"", label(inst), d.name.1));
                }
            }
        }
        for a in inst.attributes() {
            let ns = a.namespace().unwrap_or("");
            if matches!(ns, NS_XSI | NS_XML | NS_SOAP11_ENV | NS_SOAP12_ENV) {
                continue;
            }
            let range = a.range_qname();
            match decls.iter().find(|d| d.name.0 == ns && d.name.1 == a.name()) {
                Some(d) if d.prohibited => {
                    self.report(Some(range), format!("Attribute \"{}\" is not allowed on {}", a.name(), label(inst)));
                }
                Some(d) => {
                    let s = self.attr_simple(d.decl);
                    let what = format!("Attribute \"{}\"", a.name());
                    self.check_value(&what, a.value(), &s, d.fixed, a.range_value());
                }
                None if any => {}
                None => {
                    let msg = match decls.iter().find(|d| d.name.1 == a.name()) {
                        Some(d) => format!(
                            "Attribute \"{}\" is in the wrong namespace: expected {}, found {}",
                            a.name(),
                            ns_label(&d.name.0),
                            ns_label(ns)
                        ),
                        None => format!("{} has no attribute \"{}\"", label(inst), a.name()),
                    };
                    self.report(Some(range), msg);
                }
            }
        }
    }
}

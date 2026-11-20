//! Generates SoapUI-style sample XML (with `?` placeholders) from XSD definitions.

use std::collections::{HashMap, HashSet};

use roxmltree::{Node, NodeId};

use super::{QName, SchemaIndex, enclosing_schema, is_xsd, resolve_qname, xsd_children};
use crate::{NS_XSD, xml_escape};

pub const INDENT: &str = "   ";
const MAX_DEPTH: usize = 40;

enum Content {
    Empty,
    Text(String),
    Children(String),
}

/// What an element's content is defined by.
enum TypeDef<'a, 'i> {
    /// Built-in XSD type, or an unresolvable reference.
    Simple(String),
    Complex(Node<'a, 'i>),
}

pub struct SampleGenerator<'s, 'a, 'i> {
    idx: &'s SchemaIndex<'a, 'i>,
    /// Prefixes assigned so far, in order of first use: (namespace, prefix).
    assigned: Vec<(String, String)>,
    /// Prefixes the WSDL author used, so samples look familiar.
    preferred: HashMap<String, String>,
    reserved: HashSet<String>,
    /// Complex types / global elements currently being expanded (recursion guard).
    stack: Vec<NodeId>,
}

impl<'s, 'a, 'i> SampleGenerator<'s, 'a, 'i> {
    pub fn new(
        idx: &'s SchemaIndex<'a, 'i>,
        preferred: HashMap<String, String>,
        reserved: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            idx,
            assigned: Vec::new(),
            preferred,
            reserved: reserved.into_iter().collect(),
            stack: Vec::new(),
        }
    }

    /// `xmlns:p="ns"` declarations for every namespace used so far.
    pub fn namespace_declarations(&self) -> String {
        self.assigned
            .iter()
            .map(|(ns, p)| format!(" xmlns:{p}=\"{}\"", xml_escape(ns)))
            .collect()
    }

    /// Return (assigning if needed) the prefix for a namespace.
    pub fn prefix_for(&mut self, ns: &str) -> String {
        if let Some((_, p)) = self.assigned.iter().find(|(n, _)| n == ns) {
            return p.clone();
        }
        let taken = |p: &str, s: &Self| s.reserved.contains(p) || s.assigned.iter().any(|(_, q)| q == p);
        let prefix = match self.preferred.get(ns) {
            Some(p) if !p.is_empty() && !taken(p, self) => p.clone(),
            _ => (1..)
                .map(|i| format!("ns{i}"))
                .find(|p| !taken(p, self))
                .unwrap(),
        };
        self.assigned.push((ns.to_string(), prefix.clone()));
        prefix
    }

    fn qualified_name(&mut self, ns: Option<&str>, local: &str) -> String {
        match ns {
            Some(ns) if !ns.is_empty() => format!("{}:{local}", self.prefix_for(ns)),
            _ => local.to_string(),
        }
    }

    /// Write a global element (e.g. a document/literal message part) by qualified name.
    pub fn global_element(&mut self, qname: &QName, level: usize, out: &mut String) {
        match self.idx.elements.get(qname).copied() {
            Some(el) => self.element(el, level, out),
            None => {
                let q = self.qualified_name(Some(&qname.0), &qname.1);
                line(out, level, &format!("<{q}>?</{q}><!--element definition not found-->"));
            }
        }
    }

    /// Write an element with a given name whose content is defined by a type QName
    /// (used for rpc-style parts and `type=` parts).
    pub fn named_element_of_type(
        &mut self,
        ns: Option<&str>,
        name: &str,
        type_qname: &QName,
        level: usize,
        out: &mut String,
    ) {
        let q = self.qualified_name(ns, name);
        let def = self.lookup_type(type_qname);
        self.write_typed(&q, def, None, level, out);
    }

    fn element(&mut self, el: Node<'a, 'i>, level: usize, out: &mut String) {
        if level > MAX_DEPTH {
            return;
        }
        // Element reference: occurrence comes from the reference, definition from the target.
        if let Some(r) = el.attribute("ref") {
            let qn = resolve_qname(el, r);
            occurrence_comment(el, level, out);
            match self.idx.elements.get(&qn).copied() {
                Some(target) => self.element_body(target, level, out),
                None => {
                    let q = self.qualified_name(Some(&qn.0), &qn.1);
                    line(out, level, &format!("<{q}>?</{q}>"));
                }
            }
            return;
        }
        occurrence_comment(el, level, out);
        self.element_body(el, level, out);
    }

    fn element_body(&mut self, el: Node<'a, 'i>, level: usize, out: &mut String) {
        if el.attribute("abstract") == Some("true") {
            return;
        }
        let Some(name) = el.attribute("name") else { return };
        let ns = element_namespace(el);
        let q = self.qualified_name(ns.as_deref(), name);

        let is_global = el.parent().is_some_and(|p| is_xsd(&p, "schema"));
        if is_global {
            if self.stack.contains(&el.id()) {
                line(out, level, &format!("<{q}/><!--recursive element omitted-->"));
                return;
            }
            self.stack.push(el.id());
        }

        let fixed = el.attribute("fixed").or_else(|| el.attribute("default"));
        let def = if let Some(t) = el.attribute("type") {
            self.lookup_type(&resolve_qname(el, t))
        } else if let Some(ct) = xsd_children(el).find(|c| is_xsd(c, "complexType")) {
            TypeDef::Complex(ct)
        } else if let Some(st) = xsd_children(el).find(|c| is_xsd(c, "simpleType")) {
            TypeDef::Simple(self.simple_value(st, 0))
        } else {
            TypeDef::Simple("?".into())
        };
        self.write_typed(&q, def, fixed, level, out);

        if is_global {
            self.stack.pop();
        }
    }

    fn lookup_type(&self, qn: &QName) -> TypeDef<'a, 'i> {
        if qn.0 == NS_XSD {
            return TypeDef::Simple("?".into());
        }
        if let Some(ct) = self.idx.complex_types.get(qn) {
            return TypeDef::Complex(*ct);
        }
        if let Some(st) = self.idx.simple_types.get(qn) {
            return TypeDef::Simple(self.simple_value(*st, 0));
        }
        TypeDef::Simple("?".into())
    }

    fn write_typed(
        &mut self,
        q: &str,
        def: TypeDef<'a, 'i>,
        fixed: Option<&str>,
        level: usize,
        out: &mut String,
    ) {
        match def {
            TypeDef::Simple(v) => {
                let v = fixed.map(xml_escape).unwrap_or(v);
                line(out, level, &format!("<{q}>{v}</{q}>"));
            }
            TypeDef::Complex(ct) => {
                if self.stack.contains(&ct.id()) {
                    line(out, level, &format!("<{q}/><!--recursive type omitted-->"));
                    return;
                }
                self.stack.push(ct.id());
                let mut attrs = Vec::new();
                let content = self.complex_type(ct, level, &mut attrs);
                self.stack.pop();
                let attrs: String = attrs
                    .iter()
                    .map(|(n, v)| format!(" {n}=\"{v}\""))
                    .collect();
                match content {
                    Content::Empty => line(out, level, &format!("<{q}{attrs}/>")),
                    Content::Text(t) => line(out, level, &format!("<{q}{attrs}>{t}</{q}>")),
                    Content::Children(c) => {
                        line(out, level, &format!("<{q}{attrs}>"));
                        out.push_str(&c);
                        line(out, level, &format!("</{q}>"));
                    }
                }
            }
        }
    }

    /// Expand a complexType: returns its content and appends its attributes.
    fn complex_type(
        &mut self,
        ct: Node<'a, 'i>,
        level: usize,
        attrs: &mut Vec<(String, String)>,
    ) -> Content {
        let mut buf = String::new();
        let mut text: Option<String> = None;

        for c in xsd_children(ct) {
            match c.tag_name().name() {
                "sequence" | "all" | "choice" | "group" | "any" => {
                    self.particle(c, level + 1, &mut buf)
                }
                "attribute" | "attributeGroup" | "anyAttribute" => self.attribute(c, attrs, 0),
                "simpleContent" => {
                    if let Some(der) = xsd_children(c).next() {
                        text = Some(match der.attribute("base") {
                            Some(b) => match self.lookup_type(&resolve_qname(der, b)) {
                                TypeDef::Simple(v) => v,
                                TypeDef::Complex(_) => "?".into(),
                            },
                            None => "?".into(),
                        });
                        for a in xsd_children(der) {
                            self.attribute(a, attrs, 0);
                        }
                    }
                }
                "complexContent" => {
                    let Some(der) = xsd_children(c).next() else { continue };
                    if der.tag_name().name() == "extension" {
                        if let Some(b) = der.attribute("base") {
                            if let TypeDef::Complex(base) =
                                self.lookup_type(&resolve_qname(der, b))
                            {
                                if !self.stack.contains(&base.id()) {
                                    self.stack.push(base.id());
                                    match self.complex_type(base, level, attrs) {
                                        Content::Children(s) => buf.push_str(&s),
                                        Content::Text(t) => text = Some(t),
                                        Content::Empty => {}
                                    }
                                    self.stack.pop();
                                }
                            }
                        }
                    }
                    for p in xsd_children(der) {
                        match p.tag_name().name() {
                            "attribute" | "attributeGroup" | "anyAttribute" => {
                                self.attribute(p, attrs, 0)
                            }
                            _ => self.particle(p, level + 1, &mut buf),
                        }
                    }
                }
                _ => {}
            }
        }

        if !buf.is_empty() {
            Content::Children(buf)
        } else if let Some(t) = text {
            Content::Text(t)
        } else if ct.attribute("mixed") == Some("true") {
            Content::Text("?".into())
        } else {
            Content::Empty
        }
    }

    fn particle(&mut self, p: Node<'a, 'i>, level: usize, out: &mut String) {
        if level > MAX_DEPTH {
            return;
        }
        match p.tag_name().name() {
            "element" => self.element(p, level, out),
            "sequence" | "all" => {
                for c in xsd_children(p) {
                    self.particle(c, level, out);
                }
            }
            "choice" => {
                let items: Vec<_> = xsd_children(p).collect();
                line(
                    out,
                    level,
                    &format!("<!--You have a CHOICE of the next {} items at this level-->", items.len()),
                );
                for c in items {
                    self.particle(c, level, out);
                }
            }
            "group" => {
                if let Some(r) = p.attribute("ref") {
                    let qn = resolve_qname(p, r);
                    if let Some(g) = self.idx.groups.get(&qn).copied() {
                        if self.stack.contains(&g.id()) {
                            return;
                        }
                        self.stack.push(g.id());
                        for c in xsd_children(g) {
                            self.particle(c, level, out);
                        }
                        self.stack.pop();
                    }
                }
            }
            "any" => line(out, level, "<!--You may enter ANY elements at this point-->"),
            _ => {}
        }
    }

    fn attribute(&mut self, a: Node<'a, 'i>, attrs: &mut Vec<(String, String)>, depth: usize) {
        if depth > 10 || a.attribute("use") == Some("prohibited") {
            return;
        }
        match a.tag_name().name() {
            "attribute" => {
                let value = |n: Node| {
                    n.attribute("fixed")
                        .or_else(|| n.attribute("default"))
                        .map(xml_escape)
                        .unwrap_or_else(|| "?".into())
                };
                if let Some(r) = a.attribute("ref") {
                    let qn = resolve_qname(a, r);
                    let name = self.qualified_name(Some(&qn.0), &qn.1);
                    let v = self.idx.attributes.get(&qn).map(|n| value(*n)).unwrap_or("?".into());
                    attrs.push((name, v));
                } else if let Some(name) = a.attribute("name") {
                    attrs.push((name.to_string(), value(a)));
                }
            }
            "attributeGroup" => {
                if let Some(r) = a.attribute("ref") {
                    let qn = resolve_qname(a, r);
                    if let Some(g) = self.idx.attribute_groups.get(&qn).copied() {
                        for c in xsd_children(g) {
                            self.attribute(c, attrs, depth + 1);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Placeholder for a simple type: the first enumeration value if there is one, else `?`.
    fn simple_value(&self, st: Node<'a, 'i>, depth: usize) -> String {
        if depth > 10 {
            return "?".into();
        }
        if let Some(r) = xsd_children(st).find(|c| is_xsd(c, "restriction")) {
            if let Some(e) = xsd_children(r).find(|c| is_xsd(c, "enumeration")) {
                if let Some(v) = e.attribute("value") {
                    return xml_escape(v);
                }
            }
            if let Some(b) = r.attribute("base") {
                if let Some(base) = self.idx.simple_types.get(&resolve_qname(r, b)) {
                    return self.simple_value(*base, depth + 1);
                }
            }
        }
        "?".into()
    }
}

/// Namespace of an element declaration, honouring `form` / `elementFormDefault`.
fn element_namespace(el: Node) -> Option<String> {
    let schema = enclosing_schema(el)?;
    let tns = schema.attribute("targetNamespace").filter(|s| !s.is_empty())?;
    let is_global = el.parent().is_some_and(|p| is_xsd(&p, "schema"));
    let qualified = is_global
        || el
            .attribute("form")
            .or_else(|| schema.attribute("elementFormDefault"))
            == Some("qualified");
    qualified.then(|| tns.to_string())
}

fn occurrence_comment(el: Node, level: usize, out: &mut String) {
    let min = el.attribute("minOccurs").unwrap_or("1");
    let max = el.attribute("maxOccurs").unwrap_or("1");
    let many = max == "unbounded" || max.parse::<u32>().is_ok_and(|m| m > 1);
    if many {
        let c = if min == "0" { "Zero or more repetitions:" } else { "1 or more repetitions:" };
        line(out, level, &format!("<!--{c}-->"));
    } else if min == "0" {
        line(out, level, "<!--Optional:-->");
    }
}

pub fn line(out: &mut String, level: usize, s: &str) {
    for _ in 0..level {
        out.push_str(INDENT);
    }
    out.push_str(s);
    out.push('\n');
}

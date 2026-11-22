//! Lookup tables over the inline XML Schemas of a WSDL document.

pub mod datatypes;
pub mod sample;
pub mod validate;

use std::collections::HashMap;

use roxmltree::{Document, Node};

use crate::NS_XSD;

/// A namespace-qualified name: (namespace URI, local name).
pub type QName = (String, String);

/// Global schema components indexed by qualified name.
#[derive(Default)]
pub struct SchemaIndex<'a, 'i> {
    pub elements: HashMap<QName, Node<'a, 'i>>,
    pub complex_types: HashMap<QName, Node<'a, 'i>>,
    pub simple_types: HashMap<QName, Node<'a, 'i>>,
    pub groups: HashMap<QName, Node<'a, 'i>>,
    pub attribute_groups: HashMap<QName, Node<'a, 'i>>,
    pub attributes: HashMap<QName, Node<'a, 'i>>,
}

impl<'a, 'i> SchemaIndex<'a, 'i> {
    /// Index every `xs:schema` found anywhere in the given documents (inline under
    /// `wsdl:types` or standalone XSD files). Earlier documents win on name clashes.
    pub fn build(docs: &'a [Document<'i>]) -> Self {
        let mut idx = SchemaIndex::default();
        for schema in docs.iter().flat_map(|d| d.descendants()).filter(|n| is_xsd(n, "schema")) {
            let tns = schema.attribute("targetNamespace").unwrap_or("").to_string();
            for child in schema.children().filter(|n| n.is_element()) {
                if child.tag_name().namespace() != Some(NS_XSD) {
                    continue;
                }
                let Some(name) = child.attribute("name") else { continue };
                let key = (tns.clone(), name.to_string());
                let map = match child.tag_name().name() {
                    "element" => &mut idx.elements,
                    "complexType" => &mut idx.complex_types,
                    "simpleType" => &mut idx.simple_types,
                    "group" => &mut idx.groups,
                    "attributeGroup" => &mut idx.attribute_groups,
                    "attribute" => &mut idx.attributes,
                    _ => continue,
                };
                map.entry(key).or_insert(child);
            }
        }
        idx
    }
}

pub fn is_xsd(n: &Node, local: &str) -> bool {
    n.is_element() && n.tag_name().namespace() == Some(NS_XSD) && n.tag_name().name() == local
}

/// XSD element children of a node (skips annotations, text and foreign elements).
pub fn xsd_children<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(|c| {
        c.is_element()
            && c.tag_name().namespace() == Some(NS_XSD)
            && c.tag_name().name() != "annotation"
    })
}

/// Resolve a `prefix:local` attribute value against the namespaces in scope at `ctx`.
pub fn resolve_qname(ctx: Node, value: &str) -> QName {
    let value = value.trim();
    match value.split_once(':') {
        Some((prefix, local)) => (
            ctx.lookup_namespace_uri(Some(prefix)).unwrap_or("").to_string(),
            local.to_string(),
        ),
        None => (
            ctx.lookup_namespace_uri(None).unwrap_or("").to_string(),
            value.to_string(),
        ),
    }
}

/// The `xs:schema` element enclosing a node.
pub fn enclosing_schema<'a, 'i>(n: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    n.ancestors().find(|a| is_xsd(a, "schema"))
}

/// Namespace of an element declaration, honouring `form` / `elementFormDefault`.
pub fn element_namespace(el: Node) -> Option<String> {
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

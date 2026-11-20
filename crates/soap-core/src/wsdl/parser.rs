//! WSDL 1.1 parser. Works on a main document plus any imported WSDL/XSD documents
//! (see `loader.rs`); all of them contribute schema components and WSDL definitions.

use std::collections::HashMap;

use roxmltree::{Document, Node, ParsingOptions};

use super::loader::ImportedDoc;
use super::model::*;
use crate::xsd::sample::{SampleGenerator, line};
use crate::xsd::{QName, SchemaIndex, resolve_qname};
use crate::{Error, NS_WSDL, NS_WSDL_SOAP11, NS_WSDL_SOAP12, Result, xml_escape};

/// Parse a self-contained WSDL (inline schemas only).
pub fn parse_wsdl(text: &str) -> Result<WsdlDocument> {
    parse_wsdl_with_imports(text, &[])
}

pub fn parse_wsdl_with_imports(text: &str, imports: &[ImportedDoc]) -> Result<WsdlDocument> {
    let opts = || ParsingOptions { allow_dtd: true, ..Default::default() };
    let mut docs = vec![Document::parse_with_options(text, opts())?];
    {
        let root = docs[0].root_element();
        if root.tag_name().name() == "description" {
            return Err(Error::Wsdl("WSDL 2.0 documents are not supported (only WSDL 1.1)".into()));
        }
        if !is_wsdl(&root, "definitions") {
            return Err(Error::Wsdl(format!(
                "root element is <{}>, expected wsdl:definitions",
                root.tag_name().name()
            )));
        }
    }
    for imp in imports {
        let d = Document::parse_with_options(&imp.text, opts())
            .map_err(|e| Error::Xml(format!("{}: {e}", imp.location)))?;
        docs.push(d);
    }
    let root = docs[0].root_element();

    let idx = SchemaIndex::build(&docs);
    let tns = root.attribute("targetNamespace").unwrap_or("").to_string();

    // Every wsdl:definitions element: the main WSDL first, then imported WSDLs.
    let definitions: Vec<Node> = docs
        .iter()
        .map(|d| d.root_element())
        .filter(|r| is_wsdl(r, "definitions"))
        .collect();
    let def_tns = |d: Node| d.attribute("targetNamespace").unwrap_or("").to_string();

    let named = |local: &'static str| -> HashMap<QName, Node> {
        let mut map = HashMap::new();
        for d in &definitions {
            let t = def_tns(*d);
            for n in wsdl_children(*d, local) {
                if let Some(name) = n.attribute("name") {
                    map.entry((t.clone(), name.to_string())).or_insert(n);
                }
            }
        }
        map
    };
    let messages = named("message");
    let port_types = named("portType");

    // binding QName -> endpoint addresses
    let mut endpoints: HashMap<QName, Vec<String>> = HashMap::new();
    for service in definitions.iter().flat_map(|d| wsdl_children(*d, "service")) {
        for port in wsdl_children(service, "port") {
            let Some(b) = port.attribute("binding") else { continue };
            let addr = port
                .children()
                .find(|c| c.is_element() && c.tag_name().name() == "address" && is_soap_ns(c))
                .and_then(|a| a.attribute("location"));
            if let Some(addr) = addr {
                endpoints.entry(resolve_qname(port, b)).or_default().push(addr.to_string());
            }
        }
    }

    // Prefixes declared by the WSDL author, reused in generated samples (main document wins).
    let mut preferred: HashMap<String, String> = HashMap::new();
    for d in &docs {
        for ns in d.root_element().namespaces() {
            if let Some(p) = ns.name() {
                preferred.entry(ns.uri().to_string()).or_insert_with(|| p.to_string());
            }
        }
    }

    let mut interfaces = Vec::new();
    for binding in definitions.iter().flat_map(|d| wsdl_children(*d, "binding")) {
        let Some(soap_binding) = binding
            .children()
            .find(|c| c.is_element() && c.tag_name().name() == "binding" && is_soap_ns(c))
        else {
            continue; // HTTP or other non-SOAP binding
        };
        let binding_tns = def_tns(binding.document().root_element());
        let soap_ns = soap_binding.tag_name().namespace().unwrap_or_default();
        let version = if soap_ns == NS_WSDL_SOAP12 { SoapVersion::Soap12 } else { SoapVersion::Soap11 };
        let default_style = parse_style(soap_binding.attribute("style")).unwrap_or(Style::Document);
        let name = binding.attribute("name").unwrap_or("Binding").to_string();
        let port_type = binding
            .attribute("type")
            .and_then(|t| port_types.get(&resolve_qname(binding, t)).copied());

        let mut operations = Vec::new();
        for op in wsdl_children(binding, "operation") {
            let Some(op_name) = op.attribute("name") else { continue };
            let soap_op = soap_child(op, soap_ns, "operation");
            let soap_action = soap_op.and_then(|o| o.attribute("soapAction")).map(str::to_string);
            let style = soap_op
                .and_then(|o| parse_style(o.attribute("style")))
                .unwrap_or(default_style);

            let pt_op = port_type.and_then(|pt| {
                wsdl_children(pt, "operation").find(|o| o.attribute("name") == Some(op_name))
            });
            let documentation = pt_op
                .and_then(|o| wsdl_children(o, "documentation").next())
                .and_then(|d| d.text())
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            let input_msg = pt_op
                .and_then(|o| wsdl_children(o, "input").next())
                .and_then(|i| Some(resolve_qname(i, i.attribute("message")?)))
                .and_then(|q| messages.get(&q).copied());

            let input = wsdl_children(op, "input").next();
            let ctx = SampleCtx {
                idx: &idx,
                preferred: preferred.clone(),
                version,
                tns: &binding_tns,
                messages: &messages,
            };
            let sample_request = ctx.build_sample(op_name, style, input, soap_ns, input_msg);

            operations.push(Operation {
                name: op_name.to_string(),
                soap_action,
                style,
                documentation,
                sample_request,
            });
        }

        let qn = (binding_tns.clone(), name.clone());
        interfaces.push(Interface {
            name,
            soap_version: version,
            endpoints: endpoints.get(&qn).cloned().unwrap_or_default(),
            operations,
        });
    }

    if interfaces.is_empty() {
        return Err(Error::Wsdl("no SOAP 1.1 or 1.2 bindings found".into()));
    }

    Ok(WsdlDocument { target_namespace: tns, interfaces })
}

struct SampleCtx<'s, 'a, 'i> {
    idx: &'s SchemaIndex<'a, 'i>,
    preferred: HashMap<String, String>,
    version: SoapVersion,
    tns: &'s str,
    messages: &'s HashMap<QName, Node<'a, 'i>>,
}

impl<'s, 'a, 'i> SampleCtx<'s, 'a, 'i> {
    fn build_sample(
        self,
        op_name: &str,
        style: Style,
        input: Option<Node<'a, 'i>>,
        soap_ns: &str,
        input_msg: Option<Node<'a, 'i>>,
    ) -> String {
        let env = self.version.envelope_prefix();
        let mut g = SampleGenerator::new(self.idx, self.preferred, [env.to_string()]);

        let soap_body = input.and_then(|i| soap_child(i, soap_ns, "body"));
        // `parts="a b"` restricts which message parts go into the body.
        let body_parts: Option<Vec<&str>> =
            soap_body.and_then(|b| b.attribute("parts")).map(|p| p.split_whitespace().collect());
        let parts: Vec<Node> = input_msg
            .map(|m| {
                wsdl_children(m, "part")
                    .filter(|p| match (&body_parts, p.attribute("name")) {
                        (Some(list), Some(n)) => list.contains(&n),
                        _ => true,
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Headers declared with <soap:header message="..." part="..."/>.
        let mut header = String::new();
        if let Some(input) = input {
            for h in input
                .children()
                .filter(|c| c.is_element() && c.tag_name().name() == "header" && c.tag_name().namespace() == Some(soap_ns))
            {
                let (Some(m), Some(pn)) = (h.attribute("message"), h.attribute("part")) else { continue };
                let part = self
                    .messages
                    .get(&resolve_qname(h, m))
                    .and_then(|msg| wsdl_children(*msg, "part").find(|p| p.attribute("name") == Some(pn)));
                if let Some(part) = part {
                    header.push_str(&part_buf(&mut g, part, 2));
                }
            }
        }

        let mut body = String::new();
        match style {
            Style::Document => {
                for p in &parts {
                    body.push_str(&part_buf(&mut g, *p, 2));
                }
            }
            Style::Rpc => {
                let ns = soap_body
                    .and_then(|b| b.attribute("namespace"))
                    .filter(|s| !s.is_empty())
                    .unwrap_or(self.tns);
                let wrapper = format!("{}:{op_name}", g.prefix_for(ns));
                line(&mut body, 2, &format!("<{wrapper}>"));
                for p in &parts {
                    body.push_str(&part_buf(&mut g, *p, 3));
                }
                line(&mut body, 2, &format!("</{wrapper}>"));
            }
        }

        let env_ns = xml_escape(self.version.envelope_ns());
        let mut out = format!(
            "<{env}:Envelope xmlns:{env}=\"{env_ns}\"{}>\n",
            g.namespace_declarations()
        );
        if header.is_empty() {
            line(&mut out, 1, &format!("<{env}:Header/>"));
        } else {
            line(&mut out, 1, &format!("<{env}:Header>"));
            out.push_str(&header);
            line(&mut out, 1, &format!("</{env}:Header>"));
        }
        line(&mut out, 1, &format!("<{env}:Body>"));
        out.push_str(&body);
        line(&mut out, 1, &format!("</{env}:Body>"));
        out.push_str(&format!("</{env}:Envelope>"));
        out
    }
}

/// Render one message part: `element=` parts are global elements; `type=` parts
/// (rpc style) become an unqualified accessor element named after the part.
fn part_buf(g: &mut SampleGenerator, part: Node, level: usize) -> String {
    let mut buf = String::new();
    if let Some(el) = part.attribute("element") {
        g.global_element(&resolve_qname(part, el), level, &mut buf);
    } else if let Some(t) = part.attribute("type") {
        let name = part.attribute("name").unwrap_or("part");
        g.named_element_of_type(None, name, &resolve_qname(part, t), level, &mut buf);
    }
    buf
}

fn is_wsdl(n: &Node, local: &str) -> bool {
    n.is_element() && n.tag_name().namespace() == Some(NS_WSDL) && n.tag_name().name() == local
}

fn wsdl_children<'a, 'i>(n: Node<'a, 'i>, local: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |c| is_wsdl(c, local))
}

fn is_soap_ns(n: &Node) -> bool {
    matches!(n.tag_name().namespace(), Some(NS_WSDL_SOAP11) | Some(NS_WSDL_SOAP12))
}

fn soap_child<'a, 'i>(n: Node<'a, 'i>, soap_ns: &str, local: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| {
        c.is_element() && c.tag_name().namespace() == Some(soap_ns) && c.tag_name().name() == local
    })
}

fn parse_style(s: Option<&str>) -> Option<Style> {
    match s? {
        "rpc" => Some(Style::Rpc),
        "document" => Some(Style::Document),
        _ => None,
    }
}

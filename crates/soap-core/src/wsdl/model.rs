use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoapVersion {
    #[serde(rename = "1.1")]
    Soap11,
    #[serde(rename = "1.2")]
    Soap12,
}

impl SoapVersion {
    pub fn envelope_ns(self) -> &'static str {
        match self {
            SoapVersion::Soap11 => crate::NS_SOAP11_ENV,
            SoapVersion::Soap12 => crate::NS_SOAP12_ENV,
        }
    }

    pub fn envelope_prefix(self) -> &'static str {
        match self {
            SoapVersion::Soap11 => "soapenv",
            SoapVersion::Soap12 => "soap",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    Document,
    Rpc,
}

/// A parsed WSDL, flattened into what the UI needs: one interface per SOAP binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsdlDocument {
    pub target_namespace: String,
    pub interfaces: Vec<Interface>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interface {
    /// Binding local name, e.g. `CalculatorSoap`.
    pub name: String,
    pub soap_version: SoapVersion,
    /// Endpoint addresses from `service/port` elements that use this binding.
    pub endpoints: Vec<String>,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub name: String,
    pub soap_action: Option<String>,
    pub style: Style,
    pub documentation: Option<String>,
    /// A full SOAP envelope with placeholder values, ready to edit.
    pub sample_request: String,
    /// What the request Body must contain; used to validate requests against the WSDL.
    #[serde(default)]
    pub input_body: BodyShape,
}

/// A namespace-qualified XML name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlName {
    pub namespace: String,
    pub name: String,
}

/// The expected content of an operation's request `soap:Body`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyShape {
    /// rpc style: the wrapper element named after the operation; parts go inside it.
    #[serde(default)]
    pub rpc_wrapper: Option<XmlName>,
    #[serde(default)]
    pub parts: Vec<BodyPart>,
    /// `use="encoded"` (SOAP encoding): part contents are not checked against the schema.
    #[serde(default)]
    pub encoded: bool,
}

/// One message part: a global element (`element=`), or an unqualified accessor
/// element named after the part whose content has a schema type (`type=`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyPart {
    pub name: String,
    #[serde(default)]
    pub element: Option<XmlName>,
    #[serde(default)]
    pub type_name: Option<XmlName>,
}

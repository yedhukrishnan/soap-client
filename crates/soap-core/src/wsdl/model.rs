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
}

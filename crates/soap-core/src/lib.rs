//! Core SOAP client logic: WSDL parsing, sample request generation, envelope
//! handling, HTTP transport, assertions and project persistence.
//! This crate has no UI dependencies so it can be unit-tested on its own.

pub mod assertions;
pub mod envelope;
pub mod error;
pub mod http;
pub mod project;
pub mod session;
pub mod update;
pub mod vars;
pub mod wsdl;
pub mod xml_fmt;
pub mod xsd;

pub use error::{Error, Result};

pub const NS_WSDL: &str = "http://schemas.xmlsoap.org/wsdl/";
pub const NS_WSDL_SOAP11: &str = "http://schemas.xmlsoap.org/wsdl/soap/";
pub const NS_WSDL_SOAP12: &str = "http://schemas.xmlsoap.org/wsdl/soap12/";
pub const NS_XSD: &str = "http://www.w3.org/2001/XMLSchema";
pub const NS_SOAP11_ENV: &str = "http://schemas.xmlsoap.org/soap/envelope/";
pub const NS_SOAP12_ENV: &str = "http://www.w3.org/2003/05/soap-envelope";
pub const NS_WSSE: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd";
pub const NS_WSU: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd";

/// Escape text for use in XML element content or attribute values.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

//! Project files: imported WSDLs plus saved requests, stored as JSON.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::assertions::Assertion;
use crate::http::RequestSpec;
use crate::wsdl::{ImportedDoc, WsdlDocument, load_with_imports, parse_wsdl_with_imports};
use crate::{Error, Result};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    #[serde(default = "format_version")]
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub wsdls: Vec<WsdlEntry>,
    #[serde(default)]
    pub requests: Vec<SavedRequest>,
}

fn format_version() -> u32 {
    FORMAT_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsdlEntry {
    pub id: String,
    /// URL or file path the WSDL was imported from.
    pub source: String,
    /// Raw WSDL text, stored so projects work offline and re-parse identically.
    pub text: String,
    /// Documents referenced via wsdl:import / xsd:import / xsd:include, also stored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<ImportedDoc>,
    /// Parsed form; never written to disk, rebuilt on load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<WsdlDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequest {
    pub id: String,
    pub name: String,
    pub wsdl_id: String,
    pub interface_name: String,
    pub operation_name: String,
    pub spec: RequestSpec,
    #[serde(default)]
    pub assertions: Vec<Assertion>,
}

/// Load a WSDL (and everything it imports) from an http(s) URL or a local path, and parse it.
pub async fn import_wsdl(source: &str) -> Result<WsdlEntry> {
    let source = source.trim();
    let (text, imports) = load_with_imports(source).await?;
    let document = parse_wsdl_with_imports(&text, &imports)?;
    Ok(WsdlEntry {
        id: uuid::Uuid::new_v4().to_string(),
        source: source.to_string(),
        text,
        imports,
        document: Some(document),
    })
}

pub fn save(project: &Project, path: &Path) -> Result<()> {
    let mut p = project.clone();
    p.version = FORMAT_VERSION;
    for w in &mut p.wsdls {
        w.document = None;
    }
    let json = serde_json::to_string_pretty(&p).map_err(|e| Error::Project(e.to_string()))?;
    // Write to a sibling temp file then rename, so a crash never leaves a truncated project.
    let tmp = path.with_extension("tmp-save");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Project> {
    let json = std::fs::read_to_string(path)?;
    let mut p: Project = serde_json::from_str(&json).map_err(|e| Error::Project(e.to_string()))?;
    if p.version > FORMAT_VERSION {
        return Err(Error::Project(format!(
            "project format v{} is newer than this app supports (v{FORMAT_VERSION})",
            p.version
        )));
    }
    for w in &mut p.wsdls {
        w.document = Some(parse_wsdl_with_imports(&w.text, &w.imports)?);
    }
    Ok(p)
}

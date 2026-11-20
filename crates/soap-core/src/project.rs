//! Project files: imported WSDLs, saved requests, variables and environments, stored as JSON.
//! Secret variable values are never part of a project; they live in the OS keychain.

use std::collections::BTreeMap;
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
    /// Stable id; namespaces this project's secrets in the keychain.
    #[serde(default = "new_id")]
    pub id: String,
    pub name: String,
    /// Project-level variables; the active environment's variables override them.
    #[serde(default)]
    pub variables: Vec<Variable>,
    #[serde(default)]
    pub environments: Vec<Environment>,
    #[serde(default)]
    pub active_environment: Option<String>,
    #[serde(default)]
    pub wsdls: Vec<WsdlEntry>,
    #[serde(default)]
    pub requests: Vec<SavedRequest>,
}

fn format_version() -> u32 {
    FORMAT_VERSION
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    /// Plain value. Always empty for secrets (their value is in the keychain).
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub variables: Vec<Variable>,
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
    /// User-chosen display names for interfaces (binding name -> label). Requests keep
    /// referring to the real binding name, so renaming never breaks them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub interface_labels: BTreeMap<String, String>,
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
        interface_labels: BTreeMap::new(),
        document: Some(document),
    })
}

/// The on-disk form: parsed documents dropped, secret values cleared (defence in depth).
pub fn for_disk(project: &Project) -> Project {
    let mut p = project.clone();
    p.version = FORMAT_VERSION;
    for w in &mut p.wsdls {
        w.document = None;
    }
    let vars = p.variables.iter_mut().chain(p.environments.iter_mut().flat_map(|e| e.variables.iter_mut()));
    for v in vars.filter(|v| v.secret) {
        v.value.clear();
    }
    p
}

/// Re-parse the stored WSDLs after loading.
pub fn hydrate(p: &mut Project) -> Result<()> {
    if p.version > FORMAT_VERSION {
        return Err(Error::Project(format!(
            "project format v{} is newer than this app supports (v{FORMAT_VERSION})",
            p.version
        )));
    }
    for w in &mut p.wsdls {
        w.document = Some(parse_wsdl_with_imports(&w.text, &w.imports)?);
    }
    Ok(())
}

/// Write to a sibling temp file then rename, so a crash never leaves a truncated file.
pub(crate) fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    write_impl(path, contents, false)
}

/// Like [`write_atomic`], but readable by the current user only (mode 0600 on Unix).
/// Used for app-internal files such as the autosaved session.
pub fn write_private(path: &Path, contents: &str) -> Result<()> {
    write_impl(path, contents, true)
}

fn write_impl(path: &Path, contents: &str, private: bool) -> Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp-save");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut f = opts.open(&tmp)?;
    f.write_all(contents.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn save(project: &Project, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(&for_disk(project)).map_err(|e| Error::Project(e.to_string()))?;
    write_atomic(path, &json)
}

pub fn load(path: &Path) -> Result<Project> {
    let json = std::fs::read_to_string(path)?;
    let mut p: Project = serde_json::from_str(&json).map_err(|e| Error::Project(e.to_string()))?;
    hydrate(&mut p)?;
    Ok(p)
}

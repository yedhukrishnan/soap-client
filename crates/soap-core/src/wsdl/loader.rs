//! Loads a WSDL plus every document it references (`wsdl:import`, `xsd:import`,
//! `xsd:include`, `xsd:redefine`), recursively, from local files or http(s) URLs.
//! Relative locations are resolved against the referencing document.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use reqwest::Url;
use roxmltree::Document;
use serde::{Deserialize, Serialize};

use crate::{Error, NS_WSDL, NS_XSD, Result};

/// Safety limit against runaway import graphs.
const MAX_DOCUMENTS: usize = 250;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedDoc {
    /// Resolved URL or absolute file path.
    pub location: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Location {
    Url(Url),
    File(PathBuf),
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Location::Url(u) => write!(f, "{u}"),
            Location::File(p) => write!(f, "{}", p.display()),
        }
    }
}

impl Location {
    fn parse(source: &str) -> Result<Location> {
        let s = source.trim();
        if s.starts_with("http://") || s.starts_with("https://") {
            return Url::parse(s)
                .map(Location::Url)
                .map_err(|e| Error::Wsdl(format!("invalid URL '{s}': {e}")));
        }
        let path = expand_home(s.strip_prefix("file://").unwrap_or(s));
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        Ok(Location::File(path))
    }

    /// Resolve a (possibly relative) reference found inside this document.
    fn join(&self, reference: &str) -> Result<Location> {
        let r = reference.trim();
        if r.starts_with("http://") || r.starts_with("https://") || r.starts_with("file://") {
            return Location::parse(r);
        }
        match self {
            Location::Url(base) => base
                .join(r)
                .map(Location::Url)
                .map_err(|e| Error::Wsdl(format!("cannot resolve '{r}' against {base}: {e}"))),
            Location::File(base) => {
                let p = Path::new(r);
                let joined = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    base.parent().unwrap_or(Path::new(".")).join(p)
                };
                Ok(Location::File(std::fs::canonicalize(&joined).unwrap_or(joined)))
            }
        }
    }

    async fn fetch(&self) -> Result<String> {
        match self {
            Location::Url(u) => crate::http::fetch_text(u.as_str(), false).await,
            Location::File(p) => std::fs::read_to_string(p)
                .map_err(|e| Error::Io(format!("cannot read {}: {e}", p.display()))),
        }
    }
}

/// Fetch a WSDL and all documents it transitively references.
/// Returns the main WSDL text and the imported documents (in discovery order).
pub async fn load_with_imports(source: &str) -> Result<(String, Vec<ImportedDoc>)> {
    let root = Location::parse(source)?;
    let main = root.fetch().await?;

    let mut seen: HashSet<Location> = HashSet::from([root.clone()]);
    let mut queue: Vec<(Location, String)> = Vec::new();
    for r in references(&main, &root)? {
        if seen.insert(r.clone()) {
            queue.push((r, root.to_string()));
        }
    }

    let mut imports = Vec::new();
    while let Some((loc, referrer)) = (!queue.is_empty()).then(|| queue.remove(0)) {
        if imports.len() >= MAX_DOCUMENTS {
            return Err(Error::Wsdl(format!("more than {MAX_DOCUMENTS} imported documents")));
        }
        let text = loc.fetch().await.map_err(|e| {
            Error::Wsdl(format!("cannot load imported document {loc} (referenced from {referrer}): {e}"))
        })?;
        for r in references(&text, &loc)? {
            if seen.insert(r.clone()) {
                queue.push((r, loc.to_string()));
            }
        }
        imports.push(ImportedDoc { location: loc.to_string(), text });
    }
    Ok((main, imports))
}

/// Locations referenced by a WSDL or XSD document.
fn references(text: &str, base: &Location) -> Result<Vec<Location>> {
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = Document::parse_with_options(text, opts)
        .map_err(|e| Error::Xml(format!("{base}: {e}")))?;
    let mut out = Vec::new();
    for n in doc.descendants().filter(|n| n.is_element()) {
        let attr = match (n.tag_name().namespace(), n.tag_name().name()) {
            (Some(NS_XSD), "import" | "include" | "redefine") => "schemaLocation",
            (Some(NS_WSDL), "import") => "location",
            _ => continue,
        };
        if let Some(loc) = n.attribute(attr).filter(|s| !s.trim().is_empty()) {
            out.push(base.join(loc)?);
        }
    }
    Ok(out)
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => path.into(),
    }
}

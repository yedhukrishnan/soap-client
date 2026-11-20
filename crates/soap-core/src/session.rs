//! The autosaved working session: the open project (saved to its own file or not),
//! plus which request tabs were open. Restored on the next launch.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::project::{Project, for_disk, hydrate, write_atomic};
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub project: Project,
    /// Project file this session belongs to, if it has been saved.
    #[serde(default)]
    pub path: Option<String>,
    /// Edits not yet saved to `path` (or never saved).
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub open_tabs: Vec<String>,
    #[serde(default)]
    pub active_id: Option<String>,
}

pub fn save(session: &Session, path: &Path) -> Result<()> {
    let mut s = session.clone();
    s.project = for_disk(&session.project);
    let json = serde_json::to_string(&s).map_err(|e| Error::Project(e.to_string()))?;
    write_atomic(path, &json)
}

/// Load the session if there is one. A session that can't be read is moved aside
/// (`*.corrupt`) so it is not overwritten and can be inspected.
pub fn load(path: &Path) -> Result<Option<Session>> {
    let json = match std::fs::read_to_string(path) {
        Ok(j) => j,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let parsed = serde_json::from_str::<Session>(&json)
        .map_err(|e| Error::Project(e.to_string()))
        .and_then(|mut s| hydrate(&mut s.project).map(|_| s));
    match parsed {
        Ok(s) => Ok(Some(s)),
        Err(e) => {
            let _ = std::fs::rename(path, path.with_extension("corrupt"));
            Err(Error::Project(format!("previous session could not be restored: {e}")))
        }
    }
}

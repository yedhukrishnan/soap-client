//! Secret variable storage in the OS credential store (macOS Keychain, Linux Secret
//! Service). If no store is available, secrets are kept in memory for this run only.
//! Secret values never cross back to the frontend; it can only set, delete and check.
//!
//! To keep OS permission prompts to a minimum:
//! - all secrets of a project live in ONE credential ("vault:<projectId>", JSON), read
//!   at most once per app session and then served from memory;
//! - "is this secret stored?" is answered from an index of key *names* (no values) in
//!   the app data dir, plus an attribute-only search for legacy items — neither
//!   reads secret data, so neither triggers a prompt.
//!
//! Keys are "<projectId>:<scope>:<name>". Earlier versions stored one credential per
//! key; those are moved into the vault the first time they are read.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use keyring::{Entry, Error};
use serde::Serialize;

const SERVICE: &str = "dev.soapclient.app";

pub struct Secrets {
    service: &'static str,
    persistent: bool,
    index_path: OnceLock<PathBuf>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Keys known to have a stored value (names only, persisted to `index_path`).
    index: BTreeSet<String>,
    index_loaded: bool,
    /// Vaults read this session: projectId -> (scope:name -> value).
    vaults: HashMap<String, BTreeMap<String, String>>,
    /// Fallback store when no OS credential store is available.
    memory: HashMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub persistent: bool,
    pub name: &'static str,
}

impl Default for Secrets {
    fn default() -> Self {
        Self::with_service(SERVICE)
    }
}

impl Secrets {
    pub fn with_service(service: &'static str) -> Self {
        Self {
            service,
            persistent: Entry::store_status().is_ok(),
            index_path: OnceLock::new(),
            state: Mutex::default(),
        }
    }
}

fn err(e: Error) -> String {
    format!("credential store: {e}")
}

/// "<project>:<scope>:<name>" -> ("<project>", "<scope>:<name>")
fn split(key: &str) -> Result<(&str, &str), String> {
    key.split_once(':').ok_or_else(|| format!("invalid secret key '{key}'"))
}

fn vault_account(project: &str) -> String {
    format!("vault:{project}")
}

impl Secrets {
    /// Called once at startup with the app data directory.
    pub fn init(&self, data_dir: PathBuf) {
        let _ = self.index_path.set(data_dir.join("secret-index.json"));
    }

    pub fn info(&self) -> BackendInfo {
        let name = if !self.persistent {
            "memory (not saved)"
        } else if cfg!(target_os = "macos") {
            "macOS Keychain"
        } else if cfg!(windows) {
            "Windows Credential Manager"
        } else {
            "Secret Service"
        };
        BackendInfo { persistent: self.persistent, name }
    }

    // ---- index (names only) ----

    fn load_index(&self, st: &mut State) {
        if st.index_loaded {
            return;
        }
        st.index_loaded = true;
        if let Some(p) = self.index_path.get()
            && let Ok(json) = std::fs::read_to_string(p) {
                st.index = serde_json::from_str(&json).unwrap_or_default();
            }
    }

    fn save_index(&self, st: &State) {
        if let Some(p) = self.index_path.get() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_string(&st.index) {
                let _ = soap_core::project::write_private(p, &json);
            }
        }
    }

    // ---- vault (one credential per project) ----

    /// Read the project's vault (at most once per session).
    fn vault<'s>(&self, st: &'s mut State, project: &str) -> Result<&'s mut BTreeMap<String, String>, String> {
        if !st.vaults.contains_key(project) {
            let map = match Entry::new(self.service, &vault_account(project)).and_then(|e| e.get_password()) {
                Ok(json) => serde_json::from_str(&json).map_err(|e| format!("corrupt secret vault: {e}"))?,
                Err(Error::NoEntry) => BTreeMap::new(),
                Err(e) => return Err(err(e)),
            };
            for k in map.keys() {
                st.index.insert(format!("{project}:{k}"));
            }
            st.vaults.insert(project.to_string(), map);
        }
        Ok(st.vaults.get_mut(project).unwrap())
    }

    fn write_vault(&self, project: &str, map: &BTreeMap<String, String>) -> Result<(), String> {
        let entry = Entry::new(self.service, &vault_account(project)).map_err(err)?;
        if map.is_empty() {
            return match entry.delete_credential() {
                Ok(()) | Err(Error::NoEntry) => Ok(()),
                Err(e) => Err(err(e)),
            };
        }
        let json = serde_json::to_string(map).map_err(|e| e.to_string())?;
        entry.set_password(&json).map_err(err)
    }

    fn delete_legacy(&self, key: &str) {
        if let Ok(e) = Entry::new(self.service, key) {
            let _ = e.delete_credential();
        }
    }

    /// Accounts of all credentials under our service, found without reading any
    /// secret data (attribute-only search, so no permission prompt).
    fn existing_accounts(&self) -> BTreeSet<String> {
        let spec = HashMap::from([("service", self.service)]);
        keyring_core::Entry::search(&spec)
            .map(|entries| entries.iter().filter_map(|e| e.get_specifiers()).map(|(_, a)| a).collect())
            .unwrap_or_default()
    }

    // ---- public API ----

    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let mut st = self.state.lock().unwrap();
        if !self.persistent {
            return Ok(st.memory.get(key).cloned());
        }
        self.load_index(&mut st);
        let (project, inner) = split(key)?;
        if let Some(v) = self.vault(&mut st, project)?.get(inner) {
            return Ok(Some(v.clone()));
        }
        // Legacy per-key credential: move it into the vault.
        match Entry::new(self.service, key).and_then(|e| e.get_password()) {
            Ok(v) => {
                let vault = self.vault(&mut st, project)?;
                vault.insert(inner.to_string(), v.clone());
                let snapshot = vault.clone();
                self.write_vault(project, &snapshot)?;
                self.delete_legacy(key);
                st.index.insert(key.to_string());
                self.save_index(&st);
                Ok(Some(v))
            }
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(err(e)),
        }
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.persistent {
            st.memory.insert(key.into(), value.into());
            return Ok(());
        }
        self.load_index(&mut st);
        let (project, inner) = split(key)?;
        let vault = self.vault(&mut st, project)?;
        vault.insert(inner.to_string(), value.to_string());
        let snapshot = vault.clone();
        self.write_vault(project, &snapshot)?;
        self.delete_legacy(key);
        st.index.insert(key.to_string());
        self.save_index(&st);
        Ok(())
    }

    pub fn delete(&self, keys: &[String]) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.persistent {
            for k in keys {
                st.memory.remove(k);
            }
            return Ok(());
        }
        self.load_index(&mut st);
        let mut by_project: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for k in keys {
            let (p, inner) = split(k)?;
            by_project.entry(p).or_default().push(inner);
        }
        for (project, inners) in by_project {
            let vault = self.vault(&mut st, project)?;
            let mut changed = false;
            for inner in &inners {
                changed |= vault.remove(*inner).is_some();
            }
            if changed {
                let snapshot = vault.clone();
                self.write_vault(project, &snapshot)?;
            }
        }
        for k in keys {
            self.delete_legacy(k);
            st.index.remove(k);
        }
        self.save_index(&st);
        Ok(())
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        if let Some(v) = self.get(from)? {
            self.set(to, &v)?;
            self.delete(&[from.to_string()])?;
        }
        Ok(())
    }

    /// Whether any secret is stored for this project (from the names-only index).
    pub fn has_project(&self, project: &str) -> bool {
        let mut st = self.state.lock().unwrap();
        if !self.persistent {
            return st.memory.keys().any(|k| k.starts_with(&format!("{project}:")));
        }
        self.load_index(&mut st);
        let prefix = format!("{project}:");
        st.index.iter().any(|k| k.starts_with(&prefix))
            || self.existing_accounts().iter().any(|a| a.starts_with(&prefix) || *a == vault_account(project))
    }

    /// Which keys have a stored value. Never reads secret data (no prompts).
    pub fn status(&self, keys: &[String]) -> Vec<bool> {
        let mut st = self.state.lock().unwrap();
        if !self.persistent {
            return keys.iter().map(|k| st.memory.contains_key(k)).collect();
        }
        self.load_index(&mut st);
        let legacy = self.existing_accounts();
        keys.iter()
            .map(|k| {
                st.index.contains(k)
                    || legacy.contains(k)
                    || split(k).ok().is_some_and(|(p, i)| st.vaults.get(p).is_some_and(|v| v.contains_key(i)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    //! Runs against the real OS credential store under a separate test service name.
    //! Items are created by the test binary itself, so no permission prompt appears,
    //! and everything is removed at the end.
    use super::*;

    const TEST_SERVICE: &str = "dev.soapclient.test";

    fn cleanup(project: &str, keys: &[&str]) {
        for k in keys {
            let _ = Entry::new(TEST_SERVICE, k).and_then(|e| e.delete_credential());
        }
        let _ = Entry::new(TEST_SERVICE, &vault_account(project)).and_then(|e| e.delete_credential());
    }

    #[test]
    #[ignore = "uses the real OS keychain; run with --ignored"]
    fn vault_index_and_legacy_migration() {
        let project = format!("t{}", std::process::id());
        let k1 = format!("{project}:project:password");
        let k2 = format!("{project}:env1:password");
        let legacy = format!("{project}:env2:token");
        cleanup(&project, &[&k1, &k2, &legacy]);

        let dir = std::env::temp_dir().join(format!("soap-secrets-{project}"));
        let s = Secrets::with_service(TEST_SERVICE);
        assert!(s.persistent, "no OS credential store available");
        s.init(dir.clone());

        // A legacy one-credential-per-key item from an earlier version.
        Entry::new(TEST_SERVICE, &legacy).unwrap().set_password("old&value").unwrap();

        // Status without reading any data: legacy found via attribute search.
        assert_eq!(s.status(&[k1.clone(), legacy.clone()]), [false, true]);

        s.set(&k1, "p1").unwrap();
        s.set(&k2, "p2").unwrap();
        assert_eq!(s.get(&k1).unwrap().as_deref(), Some("p1"));

        // Legacy item is migrated into the vault on first read and removed.
        assert_eq!(s.get(&legacy).unwrap().as_deref(), Some("old&value"));
        assert!(matches!(Entry::new(TEST_SERVICE, &legacy).unwrap().get_password(), Err(Error::NoEntry)));

        // Everything for the project lives in ONE credential.
        let raw = Entry::new(TEST_SERVICE, &vault_account(&project)).unwrap().get_password().unwrap();
        let vault: BTreeMap<String, String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(vault.len(), 3);

        // A fresh instance (new app session) answers status from the index file alone.
        let s2 = Secrets::with_service(TEST_SERVICE);
        s2.init(dir.clone());
        assert_eq!(s2.status(&[k1.clone(), k2.clone(), legacy.clone()]), [true, true, true]);
        let index = std::fs::read_to_string(dir.join("secret-index.json")).unwrap();
        assert!(!index.contains("p1") && !index.contains("old&value"), "index must hold names only");

        // Rename and delete.
        let k3 = format!("{project}:env1:pw2");
        s2.rename(&k2, &k3).unwrap();
        assert_eq!(s2.get(&k3).unwrap().as_deref(), Some("p2"));
        assert_eq!(s2.get(&k2).unwrap(), None);
        s2.delete(&[k1.clone(), k3.clone(), legacy.clone()]).unwrap();
        assert_eq!(s2.status(&[k1.clone(), k3.clone(), legacy.clone()]), [false, false, false]);
        // Empty vault removes the credential entirely.
        assert!(matches!(Entry::new(TEST_SERVICE, &vault_account(&project)).unwrap().get_password(), Err(Error::NoEntry)));

        cleanup(&project, &[&k1, &k2, &k3, &legacy]);
        let _ = std::fs::remove_dir_all(dir);
    }
}

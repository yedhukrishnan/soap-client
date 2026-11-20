//! Secret variable storage in the OS credential store. Secret values never cross back
//! to the frontend; it can only set, delete and check.
//!
//! Stores, best first:
//! - macOS, signed release builds: the data-protection keychain with a "require user
//!   presence" access policy, so macOS asks for Touch ID (or the Mac password) to read.
//!   Needs the app's keychain entitlement (Developer ID provisioning profile).
//! - the login keychain (macOS dev/unsigned builds) or Secret Service (Linux);
//! - memory for this run only, if no credential store is available.
//!
//! To keep prompts to a minimum:
//! - all secrets of a project live in ONE credential ("vault:<projectId>", JSON), read
//!   once and then served from memory until unused for [`IDLE_LOCK`] (Touch ID store);
//! - "is this secret stored?" is answered from an index of key *names* (no values) in
//!   the app data dir, plus an attribute-only search for legacy items, neither of which
//!   reads secret data.
//!
//! Keys are "<projectId>:<scope>:<name>". Older data (one credential per key, or a vault
//! in the login keychain) is moved to the current store the first time it is read.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use keyring_core::{Entry, Error};
use serde::Serialize;

const SERVICE: &str = "dev.soapclient.app";

/// With the Touch ID store, secrets are forgotten after this long without use, so the
/// next use asks for Touch ID again.
pub const IDLE_LOCK: Duration = Duration::from_secs(15 * 60);

enum Store {
    Memory,
    /// Login keychain (macOS) / Secret Service (Linux), via keyring's default store.
    Legacy,
    /// Data-protection keychain, items readable only after user presence (Touch ID).
    #[cfg(target_os = "macos")]
    Protected(std::sync::Arc<apple_native_keyring_store::protected::Store>),
}

pub struct Secrets {
    service: &'static str,
    store: Store,
    index_path: OnceLock<PathBuf>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Keys known to have a stored value (names only, persisted to `index_path`).
    index: BTreeSet<String>,
    index_loaded: bool,
    /// Vaults read this session: projectId -> (values, last used).
    vaults: HashMap<String, (BTreeMap<String, String>, Instant)>,
    /// Fallback store when no OS credential store is available.
    memory: HashMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub persistent: bool,
    /// Reading secrets requires Touch ID / the Mac password (macOS enforces it).
    pub biometric: bool,
    pub name: &'static str,
}

impl Default for Secrets {
    fn default() -> Self {
        Self::with_service(SERVICE)
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

/// An entry in the login keychain / Secret Service.
fn legacy_entry(service: &str, account: &str) -> Result<Entry, Error> {
    keyring::Entry::new(service, account).map(|e| e.inner)
}

#[cfg(target_os = "macos")]
fn protected_store(service: &str) -> Option<std::sync::Arc<apple_native_keyring_store::protected::Store>> {
    use keyring_core::api::CredentialStoreApi;
    let store = apple_native_keyring_store::protected::Store::new().ok()?;
    // Without the keychain entitlement (unsigned/dev builds) reads just report "not
    // found", but writes fail. So probe with a write + delete of a throwaway item that
    // has the default policy: this never prompts, and only succeeds when usable.
    let probe = store.build(service, "probe:availability", None).ok()?;
    let usable = probe.set_password("").is_ok();
    let _ = probe.delete_credential();
    usable.then_some(store)
}

impl Secrets {
    pub fn with_service(service: &'static str) -> Self {
        Self::with_store(service, true)
    }

    /// `allow_protected = false` forces the login keychain (used by tests).
    fn with_store(service: &'static str, allow_protected: bool) -> Self {
        #[cfg(target_os = "macos")]
        let protected = if allow_protected { protected_store(service) } else { None };
        #[cfg(not(target_os = "macos"))]
        let _ = allow_protected;

        let store = {
            #[cfg(target_os = "macos")]
            {
                match protected {
                    Some(p) => Store::Protected(p),
                    None if keyring::Entry::store_status().is_ok() => Store::Legacy,
                    None => Store::Memory,
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                if keyring::Entry::store_status().is_ok() { Store::Legacy } else { Store::Memory }
            }
        };
        Self { service, store, index_path: OnceLock::new(), state: Mutex::default() }
    }

    /// Called once at startup with the app data directory.
    pub fn init(&self, data_dir: PathBuf) {
        let _ = self.index_path.set(data_dir.join("secret-index.json"));
    }

    fn persistent(&self) -> bool {
        !matches!(self.store, Store::Memory)
    }

    fn biometric(&self) -> bool {
        #[cfg(target_os = "macos")]
        if matches!(self.store, Store::Protected(_)) {
            return true;
        }
        false
    }

    pub fn info(&self) -> BackendInfo {
        let name = if !self.persistent() {
            "memory (not saved)"
        } else if cfg!(target_os = "macos") {
            "macOS Keychain"
        } else if cfg!(windows) {
            "Windows Credential Manager"
        } else {
            "Secret Service"
        };
        BackendInfo { persistent: self.persistent(), biometric: self.biometric(), name }
    }

    // ---- entries in the active store ----

    fn entry(&self, account: &str) -> Result<Entry, Error> {
        match &self.store {
            #[cfg(target_os = "macos")]
            Store::Protected(s) => {
                use keyring_core::api::CredentialStoreApi;
                let mods = HashMap::from([("access-policy", "RequireUserPresence")]);
                s.build(self.service, account, Some(&mods))
            }
            _ => legacy_entry(self.service, account),
        }
    }

    // ---- index (names only) ----

    fn load_index(&self, st: &mut State) {
        if st.index_loaded {
            return;
        }
        st.index_loaded = true;
        if let Some(p) = self.index_path.get()
            && let Ok(json) = std::fs::read_to_string(p)
        {
            st.index = serde_json::from_str(&json).unwrap_or_default();
        }
    }

    fn save_index(&self, st: &State) {
        if let Some(p) = self.index_path.get()
            && let Ok(json) = serde_json::to_string(&st.index)
        {
            let _ = soap_core::project::write_private(p, &json);
        }
    }

    // ---- vault (one credential per project) ----

    fn read_vault(&self, project: &str) -> Result<BTreeMap<String, String>, String> {
        let account = vault_account(project);
        match self.entry(&account).and_then(|e| e.get_password()) {
            Ok(json) => return serde_json::from_str(&json).map_err(|e| format!("corrupt secret vault: {e}")),
            Err(Error::NoEntry) => {}
            Err(e) => return Err(err(e)),
        }
        // Not in the Touch ID store yet: move a vault from the login keychain, if any.
        #[cfg(target_os = "macos")]
        if matches!(self.store, Store::Protected(_)) {
            match legacy_entry(self.service, &account).and_then(|e| e.get_password()) {
                Ok(json) => {
                    let map: BTreeMap<String, String> =
                        serde_json::from_str(&json).map_err(|e| format!("corrupt secret vault: {e}"))?;
                    self.write_vault(project, &map)?;
                    if let Ok(e) = legacy_entry(self.service, &account) {
                        let _ = e.delete_credential();
                    }
                    return Ok(map);
                }
                Err(Error::NoEntry) => {}
                Err(e) => return Err(err(e)),
            }
        }
        Ok(BTreeMap::new())
    }

    /// The project's vault, read at most once (or again after [`IDLE_LOCK`] of no use
    /// with the Touch ID store).
    fn vault<'s>(&self, st: &'s mut State, project: &str) -> Result<&'s mut BTreeMap<String, String>, String> {
        let expired = self.biometric()
            && st.vaults.get(project).is_some_and(|(_, used)| used.elapsed() >= IDLE_LOCK);
        if expired {
            st.vaults.remove(project);
        }
        if !st.vaults.contains_key(project) {
            let map = self.read_vault(project)?;
            for k in map.keys() {
                st.index.insert(format!("{project}:{k}"));
            }
            st.vaults.insert(project.to_string(), (map, Instant::now()));
        }
        let (map, used) = st.vaults.get_mut(project).unwrap();
        *used = Instant::now();
        Ok(map)
    }

    fn write_vault(&self, project: &str, map: &BTreeMap<String, String>) -> Result<(), String> {
        let entry = self.entry(&vault_account(project)).map_err(err)?;
        // Delete first: deleting never needs authentication, while updating an item
        // protected by user presence would ask for Touch ID a second time.
        match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => {}
            Err(e) => return Err(err(e)),
        }
        if map.is_empty() {
            return Ok(());
        }
        let json = serde_json::to_string(map).map_err(|e| e.to_string())?;
        entry.set_password(&json).map_err(err)
    }

    fn delete_legacy(&self, key: &str) {
        if let Ok(e) = legacy_entry(self.service, key) {
            let _ = e.delete_credential();
        }
    }

    /// Accounts of all login-keychain credentials under our service, found without
    /// reading any secret data (attribute-only search, so no permission prompt).
    fn existing_accounts(&self) -> BTreeSet<String> {
        let spec = HashMap::from([("service", self.service)]);
        Entry::search(&spec)
            .map(|entries| entries.iter().filter_map(|e| e.get_specifiers()).map(|(_, a)| a).collect())
            .unwrap_or_default()
    }

    // ---- public API ----

    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let mut st = self.state.lock().unwrap();
        if !self.persistent() {
            return Ok(st.memory.get(key).cloned());
        }
        self.load_index(&mut st);
        let (project, inner) = split(key)?;
        if let Some(v) = self.vault(&mut st, project)?.get(inner) {
            return Ok(Some(v.clone()));
        }
        // Legacy per-key credential: move it into the vault.
        match legacy_entry(self.service, key).and_then(|e| e.get_password()) {
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
        if !self.persistent() {
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
        if !self.persistent() {
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
        if !self.persistent() {
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
        if !self.persistent() {
            return keys.iter().map(|k| st.memory.contains_key(k)).collect();
        }
        self.load_index(&mut st);
        let legacy = self.existing_accounts();
        keys.iter()
            .map(|k| {
                st.index.contains(k)
                    || legacy.contains(k)
                    || split(k).ok().is_some_and(|(p, i)| st.vaults.get(p).is_some_and(|(v, _)| v.contains_key(i)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    //! Runs against the real OS credential store (login keychain) under a separate test
    //! service name. Items are created by the test binary itself, so no permission
    //! prompt appears, and everything is removed at the end.
    use super::*;

    const TEST_SERVICE: &str = "dev.soapclient.test";

    fn cleanup(project: &str, keys: &[&str]) {
        for k in keys {
            let _ = legacy_entry(TEST_SERVICE, k).and_then(|e| e.delete_credential());
        }
        let _ = legacy_entry(TEST_SERVICE, &vault_account(project)).and_then(|e| e.delete_credential());
    }

    #[test]
    fn unsigned_builds_fall_back_to_login_keychain() {
        // The test binary has no keychain entitlement, so the Touch ID store is unavailable.
        let s = Secrets::with_service(TEST_SERVICE);
        assert!(!s.biometric());
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
        let s = Secrets::with_store(TEST_SERVICE, false);
        assert!(s.persistent(), "no OS credential store available");
        s.init(dir.clone());

        // A legacy one-credential-per-key item from an earlier version.
        legacy_entry(TEST_SERVICE, &legacy).unwrap().set_password("old&value").unwrap();

        // Status without reading any data: legacy found via attribute search.
        assert_eq!(s.status(&[k1.clone(), legacy.clone()]), [false, true]);

        s.set(&k1, "p1").unwrap();
        s.set(&k2, "p2").unwrap();
        assert_eq!(s.get(&k1).unwrap().as_deref(), Some("p1"));

        // Legacy item is migrated into the vault on first read and removed.
        assert_eq!(s.get(&legacy).unwrap().as_deref(), Some("old&value"));
        assert!(matches!(legacy_entry(TEST_SERVICE, &legacy).unwrap().get_password(), Err(Error::NoEntry)));

        // Everything for the project lives in ONE credential.
        let raw = legacy_entry(TEST_SERVICE, &vault_account(&project)).unwrap().get_password().unwrap();
        let vault: BTreeMap<String, String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(vault.len(), 3);

        // A fresh instance (new app session) answers status from the index file alone.
        let s2 = Secrets::with_store(TEST_SERVICE, false);
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
        assert!(matches!(
            legacy_entry(TEST_SERVICE, &vault_account(&project)).unwrap().get_password(),
            Err(Error::NoEntry)
        ));

        cleanup(&project, &[&k1, &k2, &k3, &legacy]);
        let _ = std::fs::remove_dir_all(dir);
    }
}

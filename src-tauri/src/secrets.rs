//! Secret variable storage in the OS credential store (macOS Keychain, Linux Secret
//! Service). If no store is available, secrets are kept in memory for this run only.
//! Secret values never cross back to the frontend; it can only set, delete and check.

use std::collections::HashMap;
use std::sync::Mutex;

use keyring::{Entry, Error};
use serde::Serialize;

const SERVICE: &str = "dev.soapclient.app";

pub struct Secrets {
    persistent: bool,
    memory: Mutex<HashMap<String, String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub persistent: bool,
    pub name: &'static str,
}

impl Default for Secrets {
    fn default() -> Self {
        Self { persistent: Entry::store_status().is_ok(), memory: Mutex::default() }
    }
}

fn err(e: Error) -> String {
    format!("credential store: {e}")
}

impl Secrets {
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

    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        if !self.persistent {
            return Ok(self.memory.lock().unwrap().get(key).cloned());
        }
        match Entry::new(SERVICE, key).and_then(|e| e.get_password()) {
            Ok(v) => Ok(Some(v)),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(err(e)),
        }
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        if !self.persistent {
            self.memory.lock().unwrap().insert(key.into(), value.into());
            return Ok(());
        }
        Entry::new(SERVICE, key).and_then(|e| e.set_password(value)).map_err(err)
    }

    pub fn delete(&self, key: &str) -> Result<(), String> {
        if !self.persistent {
            self.memory.lock().unwrap().remove(key);
            return Ok(());
        }
        match Entry::new(SERVICE, key).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(e) => Err(err(e)),
        }
    }
}

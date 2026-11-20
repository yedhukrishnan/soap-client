//! Thin Tauri command layer over `soap-core`. The project lives in the frontend; the
//! backend holds in-flight cancellation handles and owns all access to secrets.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Deserialize;
use soap_core::assertions::{Assertion, AssertionResult, evaluate_all};
use soap_core::http::{RequestSpec, ResponseData};
use soap_core::project::{self, Project, WsdlEntry};
use soap_core::session::{self, Session};
use soap_core::vars::{VarValue, Vars, resolve_spec, secret_values};
use tauri::{AppHandle, Manager, State};
use tokio::sync::oneshot;

use crate::secrets::{BackendInfo, Secrets};

/// Cancellation senders for requests currently being sent, keyed by a client-chosen id.
#[derive(Default)]
pub struct InFlight(Mutex<HashMap<String, oneshot::Sender<()>>>);

type CmdResult<T> = Result<T, String>;

/// A variable as resolved by the frontend (environment overrides already applied):
/// either a plain value, or a key into the secret store.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VarBinding {
    name: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    secret_key: Option<String>,
}

fn session_path(app: &AppHandle) -> CmdResult<PathBuf> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("session.json"))
}

#[tauri::command]
pub async fn import_wsdl(source: String) -> CmdResult<WsdlEntry> {
    project::import_wsdl(&source).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn send_request(
    inflight: State<'_, InFlight>,
    secrets: State<'_, Secrets>,
    call_id: String,
    spec: RequestSpec,
    vars: Vec<VarBinding>,
) -> CmdResult<ResponseData> {
    // Only fetch secrets the request actually uses (avoids needless keychain prompts).
    let used = soap_core::vars::references(&serde_json::to_string(&spec).unwrap_or_default());

    let mut resolved = Vars::new();
    for b in vars.into_iter().filter(|b| used.contains(&b.name)) {
        let entry = match (b.secret_key, b.value) {
            (Some(key), _) => secrets.get(&key)?.map(|value| VarValue { value, secret: true }),
            (None, Some(value)) => Some(VarValue { value, secret: false }),
            (None, None) => None,
        };
        if let Some(v) = entry {
            resolved.insert(b.name, v);
        }
    }
    let spec = resolve_spec(&spec, &resolved).map_err(|e| e.to_string())?;
    let masked = secret_values(&resolved);

    let (tx, rx) = oneshot::channel();
    inflight.0.lock().unwrap().insert(call_id.clone(), tx);
    let result = tokio::select! {
        r = soap_core::http::send(&spec, &masked) => r.map_err(|e| e.to_string()),
        _ = rx => Err("Request cancelled".to_string()),
    };
    inflight.0.lock().unwrap().remove(&call_id);
    result
}

#[tauri::command]
pub fn cancel_request(inflight: State<'_, InFlight>, call_id: String) {
    if let Some(tx) = inflight.0.lock().unwrap().remove(&call_id) {
        let _ = tx.send(());
    }
}

#[tauri::command]
pub fn run_assertions(assertions: Vec<Assertion>, response: ResponseData) -> Vec<AssertionResult> {
    evaluate_all(&assertions, &response)
}

#[tauri::command]
pub fn format_xml(xml: String) -> Option<String> {
    soap_core::xml_fmt::pretty_print(&xml)
}

#[tauri::command]
pub fn save_project(path: PathBuf, project: Project) -> CmdResult<()> {
    project::save(&project, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_project(path: PathBuf) -> CmdResult<Project> {
    project::load(&path).map_err(|e| e.to_string())
}

// ---- session autosave ----

#[tauri::command]
pub async fn session_load(app: AppHandle) -> CmdResult<Option<Session>> {
    session::load(&session_path(&app)?).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_save(app: AppHandle, session: Session) -> CmdResult<()> {
    session::save(&session, &session_path(&app)?).map_err(|e| e.to_string())
}

// ---- secrets (async: keychain calls may block on an OS prompt) ----

#[tauri::command]
pub fn secret_backend(secrets: State<'_, Secrets>) -> BackendInfo {
    secrets.info()
}

#[tauri::command]
pub async fn secret_set(secrets: State<'_, Secrets>, key: String, value: String) -> CmdResult<()> {
    secrets.set(&key, &value)
}

#[tauri::command]
pub async fn secret_delete(secrets: State<'_, Secrets>, keys: Vec<String>) -> CmdResult<()> {
    keys.iter().try_for_each(|k| secrets.delete(k))
}

/// Move a secret to a new key (variable renamed). No-op if the old key has no value.
#[tauri::command]
pub async fn secret_rename(secrets: State<'_, Secrets>, from: String, to: String) -> CmdResult<()> {
    if let Some(v) = secrets.get(&from)? {
        secrets.set(&to, &v)?;
        secrets.delete(&from)?;
    }
    Ok(())
}

/// Which of these keys have a stored value. Values themselves are never returned.
#[tauri::command]
pub async fn secret_status(secrets: State<'_, Secrets>, keys: Vec<String>) -> CmdResult<Vec<bool>> {
    keys.iter().map(|k| secrets.get(k).map(|v| v.is_some())).collect()
}

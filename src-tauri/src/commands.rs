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
use soap_core::validate::{self, Issue, Target};
use soap_core::wsdl::{ImportedDoc, Operation, SoapVersion};
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

/// Project files are always `.json`. Enforced here (not only in the file dialog) so a
/// compromised webview can't use these commands to read or overwrite arbitrary files.
fn project_path(path: PathBuf) -> CmdResult<PathBuf> {
    let ok = path.is_absolute() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if ok { Ok(path) } else { Err(format!("not a project file path: {}", path.display())) }
}

/// Replace secret values (and their XML-escaped forms) in a message, e.g. an error
/// that quotes a URL containing a secret.
fn mask(msg: String, secrets: &[String]) -> String {
    secrets.iter().filter(|s| s.chars().count() >= 2).fold(msg, |m, s| {
        m.replace(s.as_str(), soap_core::http::MASK).replace(&soap_core::xml_escape(s), soap_core::http::MASK)
    })
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
    // Malformed XML is never sent (WSDL conformance problems are only warnings). Checked on
    // the template: substitution XML-escapes values, so it cannot break well-formedness.
    if let Some(issue) = validate::check_syntax(&spec.body) {
        return Err(format!("Request not sent. {}", issue.message));
    }
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
    let masked = secret_values(&resolved);
    let spec = resolve_spec(&spec, &resolved).map_err(|e| mask(e.to_string(), &masked))?;

    let (tx, rx) = oneshot::channel();
    inflight.0.lock().unwrap().insert(call_id.clone(), tx);
    let result = tokio::select! {
        r = soap_core::http::send(&spec, &masked) => r.map_err(|e| mask(e.to_string(), &masked)),
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

/// The WSDL operation a request is validated against.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationTarget {
    wsdl_text: String,
    #[serde(default)]
    imports: Vec<ImportedDoc>,
    binding_version: SoapVersion,
    operation: Operation,
    soap_version: SoapVersion,
    #[serde(default)]
    soap_action: Option<String>,
}

/// Check a request body: always for well-formedness and placeholder spelling (against the
/// defined variable *names*), and against the WSDL when a target is given. Runs on the
/// template, so no variable (or secret) values are involved.
#[tauri::command]
pub async fn validate_request(body: String, target: Option<ValidationTarget>, var_names: Option<Vec<String>>) -> Vec<Issue> {
    let target = target.as_ref().map(|t| Target {
        wsdl_text: &t.wsdl_text,
        imports: &t.imports,
        binding_version: t.binding_version,
        operation: &t.operation,
        soap_version: t.soap_version,
        soap_action: t.soap_action.as_deref(),
    });
    validate::validate_request(&body, target.as_ref(), &var_names.unwrap_or_default())
}

#[tauri::command]
pub fn save_project(path: PathBuf, project: Project) -> CmdResult<()> {
    project::save(&project, &project_path(path)?).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_project(path: PathBuf) -> CmdResult<Project> {
    project::load(&project_path(path)?).map_err(|e| e.to_string())
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
    secrets.delete(&keys)
}

/// Move a secret to a new key (variable renamed). No-op if the old key has no value.
#[tauri::command]
pub async fn secret_rename(secrets: State<'_, Secrets>, from: String, to: String) -> CmdResult<()> {
    secrets.rename(&from, &to)
}

/// Which of these keys have a stored value. Never reads secret data, so it never
/// triggers an OS permission prompt. Values themselves are never returned.
#[tauri::command]
pub async fn secret_status(secrets: State<'_, Secrets>, keys: Vec<String>) -> CmdResult<Vec<bool>> {
    Ok(secrets.status(&keys))
}

/// Whether secrets are stored on this machine for a project id. Answered from the
/// names-only index (no keychain read, no prompt). Used to warn before a project file
/// that isn't known to be yours gets access to them.
#[tauri::command]
pub async fn secret_project_has(secrets: State<'_, Secrets>, project_id: String) -> CmdResult<bool> {
    Ok(secrets.has_project(&project_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_paths_must_be_absolute_json() {
        assert!(project_path("/tmp/p.soapproj.json".into()).is_ok());
        assert!(project_path("/tmp/P.JSON".into()).is_ok());
        for bad in ["/Users/me/.zshrc", "/etc/passwd", "relative.json", "/tmp/x.json.sh"] {
            assert!(project_path(bad.into()).is_err(), "{bad}");
        }
    }

    #[test]
    fn masks_secrets_in_messages() {
        let m = mask("error sending to https://h/?key=s3cr&t and s3cr&amp;t".into(), &["s3cr&t".into(), "x".into()]);
        assert_eq!(m, "error sending to https://h/?key=•••••• and ••••••");
    }
}

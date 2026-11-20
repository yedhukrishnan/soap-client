//! Thin Tauri command layer over `soap-core`. The backend is stateless apart from
//! in-flight request cancellation; the project lives in the frontend.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use soap_core::assertions::{Assertion, AssertionResult, evaluate_all};
use soap_core::http::{RequestSpec, ResponseData};
use soap_core::project::{self, Project, WsdlEntry};
use tauri::State;
use tokio::sync::oneshot;

/// Cancellation senders for requests currently being sent, keyed by a client-chosen id.
#[derive(Default)]
pub struct InFlight(Mutex<HashMap<String, oneshot::Sender<()>>>);

type CmdResult<T> = Result<T, String>;

#[tauri::command]
pub async fn import_wsdl(source: String) -> CmdResult<WsdlEntry> {
    project::import_wsdl(&source).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn send_request(
    inflight: State<'_, InFlight>,
    call_id: String,
    spec: RequestSpec,
) -> CmdResult<ResponseData> {
    let (tx, rx) = oneshot::channel();
    inflight.0.lock().unwrap().insert(call_id.clone(), tx);
    let result = tokio::select! {
        r = soap_core::http::send(&spec) => r.map_err(|e| e.to_string()),
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

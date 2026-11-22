//! Update checks. The frontend asks for the automatic check (at most daily) after launch;
//! "Check for Updates…" in the menu runs one right away and reports back with an
//! `update-status` event. Only the fixed releases page can be opened from here.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use soap_core::update::{self, MANIFEST_URL, UpdateInfo, UpdatePrefs};
use tauri::menu::MenuItemKind;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_opener::OpenerExt;

pub const RELEASES_URL: &str = "https://github.com/yedhukrishnan/soap-client/releases/latest";
pub const CHECK_NOW_ID: &str = "update.check";
pub const AUTO_CHECK_ID: &str = "update.auto";

/// Serializes read-modify-write of the preferences file.
#[derive(Default)]
pub struct Updates(Mutex<()>);

fn prefs_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("updates.json"))
}

pub fn prefs<R: Runtime>(app: &AppHandle<R>) -> UpdatePrefs {
    prefs_path(app).map(|p| UpdatePrefs::load(&p)).unwrap_or_default()
}

fn modify<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&mut UpdatePrefs)) -> Result<UpdatePrefs, String> {
    let state = app.state::<Updates>();
    let _guard = state.0.lock().unwrap();
    let path = prefs_path(app).ok_or("no app data directory")?;
    let mut prefs = UpdatePrefs::load(&path);
    f(&mut prefs);
    prefs.save(&path).map_err(|e| e.to_string())?;
    Ok(prefs)
}

fn current_version<R: Runtime>(app: &AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The automatic check: does nothing if turned off or already done in the last day.
/// Failures are silent (no network is normal); it simply runs again next launch.
#[tauri::command]
pub async fn update_check_auto(app: AppHandle) -> Option<UpdateInfo> {
    if !prefs(&app).due(now()) {
        return None;
    }
    let found = update::check(MANIFEST_URL, &current_version(&app)).await.ok()?;
    let prefs = modify(&app, |p| p.last_check = Some(now())).ok()?;
    prefs.filter(found)
}

/// Don't offer this version again automatically (security releases are still shown).
#[tauri::command]
pub fn update_skip(app: AppHandle, version: String) -> Result<(), String> {
    if update::parse_version(&version).is_none() {
        return Err(format!("not a version: {version}"));
    }
    modify(&app, |p| p.skipped_version = Some(version)).map(|_| ())
}

#[tauri::command]
pub fn update_open(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(RELEASES_URL, None::<&str>).map_err(|e| e.to_string())
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Status {
    Available { update: UpdateInfo },
    Current { version: String },
    Failed { message: String },
}

/// "Check for Updates…": always checks, ignores a skipped version, and reports the
/// outcome either way.
pub fn check_now<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let current = current_version(&app);
        let status = match update::check(MANIFEST_URL, &current).await {
            Ok(Some(update)) => Status::Available { update },
            Ok(None) => Status::Current { version: current },
            Err(e) => Status::Failed { message: e.to_string() },
        };
        let _ = app.emit("update-status", status);
    });
}

/// "Check for Updates Automatically" was clicked: flip the preference and make the
/// checkmark match it.
pub fn toggle_auto<R: Runtime>(app: &AppHandle<R>) {
    let Ok(prefs) = modify(app, |p| p.auto_check = !p.auto_check) else { return };
    let items = app.menu().and_then(|m| m.items().ok()).unwrap_or_default();
    if let Some(MenuItemKind::Check(item)) = crate::menu::find(&items, AUTO_CHECK_ID) {
        let _ = item.set_checked(prefs.auto_check);
    }
}

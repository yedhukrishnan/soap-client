//! Light / Dark / System appearance, chosen in the menu and remembered per machine.
//!
//! The choice is applied twice: to the native windows (title bar, native controls, and on
//! macOS the web view's `prefers-color-scheme`), and by the frontend, which colours the
//! page itself on every platform. The frontend learns it from `appearance_get` at launch
//! and from `appearance` events.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::menu::MenuItemKind;
use tauri::{AppHandle, Emitter, Manager, Runtime, Theme};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// Menu item id and label for each choice.
pub const ITEMS: [(Appearance, &str, &str); 3] = [
    (Appearance::System, "appearance.system", "System"),
    (Appearance::Light, "appearance.light", "Light"),
    (Appearance::Dark, "appearance.dark", "Dark"),
];

fn path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("appearance.json"))
}

pub fn load<R: Runtime>(app: &AppHandle<R>) -> Appearance {
    path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save<R: Runtime>(app: &AppHandle<R>, a: Appearance) {
    if let (Some(p), Ok(json)) = (path(app), serde_json::to_string(&a)) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, json);
    }
}

/// Theme the native windows (`None` follows the system).
pub fn apply_to_windows<R: Runtime>(app: &AppHandle<R>, a: Appearance) {
    let theme = match a {
        Appearance::System => None,
        Appearance::Light => Some(Theme::Light),
        Appearance::Dark => Some(Theme::Dark),
    };
    for w in app.webview_windows().values() {
        let _ = w.set_theme(theme);
    }
}

/// A choice was clicked in the menu: remember it, apply it, and fix the checkmarks
/// (a clicked check item toggles itself).
pub fn select<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let Some((a, _, _)) = ITEMS.iter().find(|(_, item_id, _)| *item_id == id) else { return };
    save(app, *a);
    apply_to_windows(app, *a);
    let items = app.menu().and_then(|m| m.items().ok()).unwrap_or_default();
    for (choice, item_id, _) in ITEMS {
        if let Some(MenuItemKind::Check(item)) = crate::menu::find(&items, item_id) {
            let _ = item.set_checked(choice == *a);
        }
    }
    let _ = app.emit("appearance", a);
}

#[tauri::command]
pub fn appearance_get(app: AppHandle) -> Appearance {
    load(&app)
}

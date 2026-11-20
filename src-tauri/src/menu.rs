//! Application menu: the standard menus, a clean About panel, and a Help menu with
//! links. Links are opened from Rust, so the webview needs no opener permission.

use tauri::menu::{AboutMetadata, CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};
use tauri_plugin_opener::OpenerExt;

use crate::updates;

pub const AUTHOR: &str = "Yedhu Krishnan";
pub const EMAIL: &str = "dev@yedhu.me";
pub const WEBSITE: &str = "https://yedhu.me";
pub const REPOSITORY: &str = "https://github.com/yedhukrishnan/soap-client";

/// Help-menu item ids and the URL each opens.
const LINKS: &[(&str, &str, &str)] = &[
    ("help.source", "Source Code on GitHub", REPOSITORY),
    ("help.issue", "Report an Issue…", "https://github.com/yedhukrishnan/soap-client/issues"),
    ("help.website", "yedhu.me", WEBSITE),
    ("help.email", "Email dev@yedhu.me", "mailto:dev@yedhu.me"),
];

fn about_metadata<'a, R: Runtime>(app: &'a AppHandle<R>) -> AboutMetadata<'a> {
    let pkg = app.package_info();
    AboutMetadata {
        name: Some(pkg.name.clone()),
        version: Some(pkg.version.to_string()),
        // macOS: an empty build version hides the "(0.1.0)" that would repeat the version.
        short_version: if cfg!(target_os = "macos") { Some(String::new()) } else { None },
        copyright: Some(format!("© 2026 {AUTHOR} · MIT License")),
        // Shown by the Linux/Windows about dialogs (macOS ignores them).
        authors: Some(vec![format!("{AUTHOR} <{EMAIL}>")]),
        website: Some(WEBSITE.into()),
        website_label: Some("yedhu.me".into()),
        comments: Some("A lightweight SOAP client".into()),
        license: Some("MIT".into()),
        icon: app.default_window_icon().cloned(),
        ..Default::default()
    }
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = PredefinedMenuItem::about(app, None, Some(about_metadata(app)))?;
    let check_now = MenuItem::with_id(app, updates::CHECK_NOW_ID, "Check for Updates…", true, None::<&str>)?;
    let auto_check = CheckMenuItem::with_id(
        app,
        updates::AUTO_CHECK_ID,
        "Check for Updates Automatically",
        true,
        updates::prefs(app).auto_check,
        None::<&str>,
    )?;

    let link = |i: usize| {
        let (id, label, _) = LINKS[i];
        MenuItem::with_id(app, id, label, true, None::<&str>)
    };
    let help_menu = Submenu::with_items(
        app,
        "Help",
        true,
        &[
            &link(0)?,
            &link(1)?,
            &PredefinedMenuItem::separator(app)?,
            &link(2)?,
            &link(3)?,
            #[cfg(not(target_os = "macos"))]
            &PredefinedMenuItem::separator(app)?,
            #[cfg(not(target_os = "macos"))]
            &check_now,
            #[cfg(not(target_os = "macos"))]
            &auto_check,
            #[cfg(not(target_os = "macos"))]
            &PredefinedMenuItem::separator(app)?,
            #[cfg(not(target_os = "macos"))]
            &about,
        ],
    )?;

    #[cfg(target_os = "macos")]
    let app_menu = Submenu::with_items(
        app,
        app.package_info().name.clone(),
        true,
        &[
            &about,
            &check_now,
            &auto_check,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;

    #[cfg(not(target_os = "macos"))]
    let file_menu = Submenu::with_items(app, "File", true, &[&PredefinedMenuItem::quit(app, None)?])?;

    let edit_menu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;

    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    #[cfg(target_os = "macos")]
    {
        let view_menu = Submenu::with_items(app, "View", true, &[&PredefinedMenuItem::fullscreen(app, None)?])?;
        Menu::with_items(app, &[&app_menu, &edit_menu, &view_menu, &window_menu, &help_menu])
    }
    #[cfg(not(target_os = "macos"))]
    {
        Menu::with_items(app, &[&file_menu, &edit_menu, &window_menu, &help_menu])
    }
}

/// Open the URL behind a Help-menu item (only the fixed URLs above can be opened), or
/// handle the update items.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if event.id() == updates::CHECK_NOW_ID {
        updates::check_now(app);
    } else if event.id() == updates::AUTO_CHECK_ID {
        updates::toggle_auto(app);
    } else if let Some((_, _, url)) = LINKS.iter().find(|(id, _, _)| event.id() == *id) {
        let _ = app.opener().open_url(*url, None::<&str>);
    }
}

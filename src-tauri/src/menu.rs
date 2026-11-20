//! Application menu: Tauri's default menu, with a filled-in About panel.

use tauri::menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

pub const AUTHOR: &str = "Yedhu Krishnan";
pub const WEBSITE: &str = "https://yedhu.me";
pub const EMAIL: &str = "dev@yedhu.me";

fn about_metadata<'a, R: Runtime>(app: &'a AppHandle<R>) -> AboutMetadata<'a> {
    let pkg = app.package_info();
    AboutMetadata {
        name: Some(pkg.name.clone()),
        version: Some(pkg.version.to_string()),
        // macOS shows name, version, copyright and credits; the fields below the
        // credits are used by the Linux/Windows about dialogs.
        copyright: Some(format!("© 2026 {AUTHOR}")),
        credits: Some(format!("A lightweight SOAP client.\n\n{AUTHOR}\nyedhu.me\n{EMAIL}")),
        authors: Some(vec![format!("{AUTHOR} <{EMAIL}>")]),
        website: Some(WEBSITE.into()),
        website_label: Some("yedhu.me".into()),
        comments: Some("A lightweight SOAP client".into()),
        icon: app.default_window_icon().cloned(),
        ..Default::default()
    }
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = PredefinedMenuItem::about(app, None, Some(about_metadata(app)))?;

    #[cfg(target_os = "macos")]
    let app_menu = Submenu::with_items(
        app,
        app.package_info().name.clone(),
        true,
        &[
            &about,
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
        Menu::with_items(app, &[&app_menu, &edit_menu, &view_menu, &window_menu])
    }
    #[cfg(not(target_os = "macos"))]
    {
        let help_menu = Submenu::with_items(app, "Help", true, &[&about])?;
        Menu::with_items(app, &[&file_menu, &edit_menu, &window_menu, &help_menu])
    }
}

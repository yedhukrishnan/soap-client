mod commands;
mod menu;
mod secrets;
mod updates;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .menu(menu::build)
        .on_menu_event(menu::on_event)
        .setup(|app| {
            use tauri::Manager;
            if let Ok(dir) = app.path().app_data_dir() {
                app.state::<secrets::Secrets>().init(dir);
            }
            Ok(())
        })
        .manage(commands::InFlight::default())
        .manage(secrets::Secrets::default())
        .manage(updates::Updates::default())
        .invoke_handler(tauri::generate_handler![
            commands::import_wsdl,
            commands::send_request,
            commands::cancel_request,
            commands::run_assertions,
            commands::format_xml,
            commands::validate_request,
            commands::save_project,
            commands::open_project,
            commands::session_load,
            commands::session_save,
            commands::secret_backend,
            commands::secret_set,
            commands::secret_delete,
            commands::secret_rename,
            commands::secret_status,
            commands::secret_project_has,
            updates::update_check_auto,
            updates::update_skip,
            updates::update_open,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SOAP client");
}

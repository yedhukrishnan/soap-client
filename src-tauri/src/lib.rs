mod commands;
mod secrets;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::InFlight::default())
        .manage(secrets::Secrets::default())
        .invoke_handler(tauri::generate_handler![
            commands::import_wsdl,
            commands::send_request,
            commands::cancel_request,
            commands::run_assertions,
            commands::format_xml,
            commands::save_project,
            commands::open_project,
            commands::session_load,
            commands::session_save,
            commands::secret_backend,
            commands::secret_set,
            commands::secret_delete,
            commands::secret_rename,
            commands::secret_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SOAP client");
}

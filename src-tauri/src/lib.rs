mod commands;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::InFlight::default())
        .invoke_handler(tauri::generate_handler![
            commands::import_wsdl,
            commands::send_request,
            commands::cancel_request,
            commands::run_assertions,
            commands::format_xml,
            commands::save_project,
            commands::open_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SOAP client");
}

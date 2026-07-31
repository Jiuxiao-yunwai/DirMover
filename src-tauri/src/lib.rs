mod commands;
mod error;
mod models;
mod platform;
mod services;
mod state;
mod storage;
mod tasks;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(state::AppState::new().expect("failed to initialize application state"))
        .invoke_handler(tauri::generate_handler![
            commands::system::health_check,
            commands::system::get_bootstrap_data,
            commands::system::save_settings,
            commands::system::plan_migration,
            commands::system::open_path,
            commands::system::request_elevation,
            commands::scan::list_child_folders,
            commands::scan::start_scan,
            commands::scan::cancel_task,
            commands::scan::get_task,
            commands::scan::query_scan_result,
            commands::scan::detect_links,
            commands::migration::start_migration,
            commands::migration::start_restore,
            commands::migration::recover_move,
            commands::migration::list_moves
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DirMover");
}

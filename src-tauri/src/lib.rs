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
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![commands::system::health_check])
        .run(tauri::generate_context!())
        .expect("failed to run DirMover");
}

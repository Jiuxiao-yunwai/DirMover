use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatus {
    app: &'static str,
    version: &'static str,
    platform: &'static str,
}

#[tauri::command]
pub fn health_check() -> HealthStatus {
    HealthStatus {
        app: "DirMover",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
    }
}

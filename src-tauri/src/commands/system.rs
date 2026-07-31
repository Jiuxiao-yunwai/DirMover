use crate::{
    error::{AppError, AppResult},
    models::{AppSettings, BootstrapData, MigrationPlan},
    platform,
    services::{migration, storage_planner},
    state::AppState,
};
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(serde::Serialize)]
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

#[tauri::command]
pub async fn get_bootstrap_data(state: State<'_, AppState>) -> AppResult<BootstrapData> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let settings = state.storage().load_settings()?;
        let mut links = state.storage().load_links()?;
        links.sort_by(|left, right| right.detected_at_utc.cmp(&left.detected_at_utc));
        Ok(BootstrapData {
            managed_locations: storage_planner::managed_locations(&settings),
            drives: platform::fixed_ntfs_drives(),
            moves: migration::move_rows(state.storage())?,
            links,
            is_administrator: platform::is_administrator(),
            default_scan_root: storage_planner::default_scan_root(),
            automatic_managed_root: storage_planner::automatic_managed_root()
                .map(|path| path.to_string_lossy().into_owned()),
            data_directory: state.storage().paths().root.to_string_lossy().into_owned(),
            settings,
        })
    })
    .await
    .map_err(|error| AppError::Task(format!("初始化任务异常结束：{error}")))?
}

#[tauri::command]
pub async fn save_settings(
    settings: AppSettings,
    state: State<'_, AppState>,
) -> AppResult<BootstrapData> {
    for root in settings
        .managed_root
        .iter()
        .chain(settings.category_roots.values())
    {
        storage_planner::validate_managed_root(Path::new(root), None)?;
    }
    state.storage().save_settings(&settings)?;
    get_bootstrap_data(state).await
}

#[tauri::command]
pub async fn plan_migration(
    source: String,
    custom_root: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<MigrationPlan> {
    let source_path = PathBuf::from(&source);
    if !source_path.is_dir() {
        return Err(AppError::NotFound("源目录不存在。".to_string()));
    }
    let category = storage_planner::category_for_source(&source_path);
    let settings = state.storage().load_settings()?;
    let root = custom_root
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| storage_planner::category_root(&settings, &category));
    let Some(root) = root else {
        return Ok(MigrationPlan {
            source,
            category_key: category.key.to_string(),
            category_label: category.label.to_string(),
            managed_root: None,
            destination: None,
            available: false,
            reason: Some("没有可用的其他固定 NTFS 分区。".to_string()),
        });
    };

    if let Err(error) = storage_planner::validate_managed_root(&root, Some(&source_path)) {
        return Ok(MigrationPlan {
            source,
            category_key: category.key.to_string(),
            category_label: category.label.to_string(),
            managed_root: Some(root.to_string_lossy().into_owned()),
            destination: None,
            available: false,
            reason: Some(error.to_string()),
        });
    }
    let destination = if custom_root.is_some() {
        storage_planner::destination_under_base(&root, &source_path)?
    } else {
        storage_planner::destination_under_category(&root, &source_path, &category)?
    };
    Ok(MigrationPlan {
        source,
        category_key: category.key.to_string(),
        category_label: category.label.to_string(),
        managed_root: Some(root.to_string_lossy().into_owned()),
        destination: Some(destination.to_string_lossy().into_owned()),
        available: true,
        reason: None,
    })
}

#[tauri::command]
pub fn open_path(path: String, select: bool) -> AppResult<()> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err(AppError::NotFound("要打开的文件或目录不存在。".to_string()));
    }
    platform::open_in_file_manager(&path, select)
}

#[tauri::command]
pub fn request_elevation() -> AppResult<bool> {
    if platform::is_administrator() {
        return Ok(false);
    }
    platform::relaunch_elevated(&[])
}

use crate::{
    error::{AppError, AppResult},
    models::{AppSettings, AppSettingsDto, BootstrapData, MigrationPlan},
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
        let drives = platform::fixed_ntfs_drives();
        let automatic_root = storage_planner::automatic_managed_root_for(&drives);
        let mut links = state.storage().load_links()?;
        links.sort_by(|left, right| right.detected_at_utc.cmp(&left.detected_at_utc));
        Ok(BootstrapData {
            version: env!("CARGO_PKG_VERSION").to_string(),
            managed_locations: storage_planner::managed_locations(
                &settings,
                automatic_root.as_deref(),
            ),
            drives,
            moves: migration::move_rows(state.storage())?,
            links,
            is_administrator: platform::is_administrator(),
            default_scan_root: storage_planner::default_scan_root(),
            scan_presets: storage_planner::scan_presets(),
            startup_scan_root: startup_scan_root(),
            automatic_managed_root: automatic_root.map(|path| path.to_string_lossy().into_owned()),
            data_directory: state.storage().paths().root.to_string_lossy().into_owned(),
            settings: settings.into(),
        })
    })
    .await
    .map_err(|error| AppError::Task(format!("初始化任务异常结束：{error}")))?
}

#[tauri::command]
pub async fn save_settings(
    settings: AppSettingsDto,
    state: State<'_, AppState>,
) -> AppResult<BootstrapData> {
    let settings = AppSettings::from(settings);
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
pub fn open_managed_root(state: State<'_, AppState>) -> AppResult<()> {
    let settings = state.storage().load_settings()?;
    let root = settings
        .managed_root
        .map(PathBuf::from)
        .or_else(storage_planner::automatic_managed_root)
        .ok_or_else(|| AppError::InvalidInput("当前没有可用的其他固定 NTFS 分区。".to_string()))?;
    storage_planner::validate_managed_root(&root, None)?;
    std::fs::create_dir_all(&root)?;
    platform::open_in_file_manager(&root, false)
}

#[tauri::command]
pub fn open_diagnostics(state: State<'_, AppState>) -> AppResult<()> {
    let root = state.storage().paths().root.join("logs");
    std::fs::create_dir_all(&root)?;
    platform::open_in_file_manager(&root, false)
}

#[tauri::command]
pub fn request_elevation(scan_root: Option<String>) -> AppResult<bool> {
    if platform::is_administrator() {
        return Ok(false);
    }
    let arguments = if let Some(root) = scan_root {
        let path = PathBuf::from(&root);
        if !path.is_dir() {
            return Err(AppError::InvalidInput("提权扫描目录不存在。".to_string()));
        }
        vec!["--scan".to_string(), root]
    } else {
        Vec::new()
    };
    platform::relaunch_elevated(&arguments)
}

fn startup_scan_root() -> Option<String> {
    let arguments = std::env::args().collect::<Vec<_>>();
    arguments
        .iter()
        .position(|argument| argument == "--scan")
        .and_then(|index| arguments.get(index + 1))
        .filter(|path| Path::new(path).is_dir())
        .cloned()
}

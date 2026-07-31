use serde::Serialize;

use super::{AppSettingsDto, LinkRow, ManagedCategoryRow, MoveRow};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub root: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub fixed: bool,
    pub ready: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScanPreset {
    pub key: String,
    pub label: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapData {
    pub settings: AppSettingsDto,
    pub managed_locations: Vec<ManagedCategoryRow>,
    pub drives: Vec<DriveInfo>,
    pub moves: Vec<MoveRow>,
    pub links: Vec<LinkRow>,
    pub is_administrator: bool,
    pub default_scan_root: String,
    pub scan_presets: Vec<ScanPreset>,
    pub startup_scan_root: Option<String>,
    pub automatic_managed_root: Option<String>,
    pub data_directory: String,
}

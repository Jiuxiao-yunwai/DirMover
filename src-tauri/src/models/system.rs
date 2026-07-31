use serde::Serialize;

use super::{AppSettings, LinkRow, ManagedCategoryRow, MoveRow};

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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapData {
    pub settings: AppSettings,
    pub managed_locations: Vec<ManagedCategoryRow>,
    pub drives: Vec<DriveInfo>,
    pub moves: Vec<MoveRow>,
    pub links: Vec<LinkRow>,
    pub is_administrator: bool,
    pub default_scan_root: String,
    pub automatic_managed_root: Option<String>,
    pub data_directory: String,
}

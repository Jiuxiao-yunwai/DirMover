use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    pub name: String,
    pub full_path: String,
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub file_count: u64,
    pub link_count: u64,
    pub error_count: u64,
    pub is_link: bool,
    pub link_target: Option<String>,
    pub note: String,
    pub measured: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct LinkRecord {
    pub kind: String,
    pub path: String,
    pub target: String,
    pub state: String,
    #[serde(default)]
    pub scan_root: String,
    #[serde(default)]
    pub detected_at_utc: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinkRow {
    pub kind: String,
    pub path: String,
    pub target: String,
    pub state: String,
    pub scan_root: String,
    pub detected_at_utc: Option<DateTime<Utc>>,
}

impl From<LinkRecord> for LinkRow {
    fn from(record: LinkRecord) -> Self {
        Self {
            kind: record.kind,
            path: record.path,
            target: record.target,
            state: record.state,
            scan_root: record.scan_root,
            detected_at_utc: record.detected_at_utc,
        }
    }
}

impl From<LinkRow> for LinkRecord {
    fn from(row: LinkRow) -> Self {
        Self {
            kind: row.kind,
            path: row.path,
            target: row.target,
            state: row.state,
            scan_root: row.scan_root,
            detected_at_utc: row.detected_at_utc,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScanSnapshot {
    pub id: Uuid,
    pub root: String,
    pub folders: Vec<FolderRow>,
    pub links: Vec<LinkRow>,
    pub engine: String,
    pub created_at_utc: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SortField {
    Name,
    Bytes,
    #[default]
    AllocatedBytes,
    FileCount,
    Status,
}

#[derive(Clone, Copy, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Ascending,
    #[default]
    Descending,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanQuery {
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub sort_field: SortField,
    #[serde(default)]
    pub sort_direction: SortDirection,
}

const fn default_page() -> usize {
    1
}

const fn default_page_size() -> usize {
    200
}

impl Default for ScanQuery {
    fn default() -> Self {
        Self {
            page: default_page(),
            page_size: default_page_size(),
            sort_field: SortField::default(),
            sort_direction: SortDirection::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LinkRecord, LinkRow};

    #[test]
    fn separates_legacy_link_storage_from_camel_case_ipc() {
        let legacy = r#"{
          "Kind":"目录联接",
          "Path":"C:\\source",
          "Target":"D:\\target",
          "State":"有效",
          "ScanRoot":"C:\\",
          "DetectedAtUtc":null
        }"#;
        let record: LinkRecord = serde_json::from_str(legacy).expect("legacy link should parse");
        let row = LinkRow::from(record);
        let value = serde_json::to_value(row).expect("IPC row should serialize");
        assert_eq!(value["path"], r"C:\source");
        assert!(value.get("Path").is_none());
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanPage {
    pub result_id: Uuid,
    pub root: String,
    pub engine: String,
    pub created_at_utc: DateTime<Utc>,
    pub items: Vec<FolderRow>,
    pub total_items: usize,
    pub page: usize,
    pub page_size: usize,
    pub total_bytes: u64,
    pub total_allocated_bytes: u64,
    pub total_files: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub percentage: u8,
    pub phase: String,
    pub message: String,
}

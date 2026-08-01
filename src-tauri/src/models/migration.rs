use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRequest {
    pub source: String,
    pub destination: String,
}

pub mod stage {
    pub const PREPARING: &str = "Preparing";
    pub const CUTOVER_PLANNED: &str = "CutoverPlanned";
    pub const SOURCE_RENAMED: &str = "SourceRenamed";
    pub const JUNCTION_PLANNED: &str = "JunctionPlanned";
    pub const JUNCTION_CREATED: &str = "JunctionCreated";
    pub const ACTIVE: &str = "Active";
    pub const CLEANUP_PENDING: &str = "CleanupPending";
    pub const ROLLBACK_FAILED: &str = "RollbackFailed";
    pub const RESTORING: &str = "Restoring";
    pub const RESTORE_CLEANUP_PENDING: &str = "RestoreCleanupPending";
    pub const RESTORED: &str = "Restored";

    /// Stages that may need recovery intervention
    #[allow(dead_code)]
    pub const RECOVERY_STAGES: &[&str] = &[
        PREPARING,
        CUTOVER_PLANNED,
        SOURCE_RENAMED,
        JUNCTION_PLANNED,
        JUNCTION_CREATED,
        ROLLBACK_FAILED,
        RESTORING,
        RESTORE_CLEANUP_PENDING,
    ];
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct MoveRecord {
    pub id: Uuid,
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub backup_path: Option<String>,
    pub created_at_utc: DateTime<Utc>,
    #[serde(default)]
    pub destination_file_id: Option<u64>,
    #[serde(default)]
    pub destination_volume_serial: Option<u64>,
    #[serde(default)]
    pub final_sync_attempts: Option<u32>,
    #[serde(default)]
    pub stage: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MoveRow {
    pub id: Uuid,
    pub status: String,
    pub source: String,
    pub destination: String,
    pub created_at_utc: DateTime<Utc>,
    pub stage: String,
    pub recovery_available: bool,
}

#[cfg(test)]
mod tests {
    use super::MoveRecord;

    #[test]
    fn preserves_legacy_move_shape() {
        let json = r#"{
          "Id":"18ff2340-52cb-4a55-b2e6-f9d45b2c3c11",
          "Source":"C:\\source",
          "Destination":"D:\\target",
          "BackupPath":null,
          "CreatedAtUtc":"2026-07-30T12:30:00Z",
          "Stage":"Active"
        }"#;
        let record: MoveRecord = serde_json::from_str(json).expect("legacy move should parse");
        assert_eq!(record.stage, "Active");
        let value = serde_json::to_value(record).expect("move should serialize");
        assert!(value.get("CreatedAtUtc").is_some());
        assert!(value.get("createdAtUtc").is_none());
    }

    #[test]
    fn new_fields_default_to_none_for_legacy_records() {
        let json = r#"{
          "Id":"18ff2340-52cb-4a55-b2e6-f9d45b2c3c11",
          "Source":"C:\\source",
          "Destination":"D:\\target",
          "BackupPath":null,
          "CreatedAtUtc":"2026-07-30T12:30:00Z",
          "Stage":"Active"
        }"#;
        let record: MoveRecord = serde_json::from_str(json).expect("legacy move should parse");
        assert_eq!(record.destination_file_id, None);
        assert_eq!(record.destination_volume_serial, None);
        assert_eq!(record.final_sync_attempts, None);
        assert_eq!(record.backup_path, None);
    }
}

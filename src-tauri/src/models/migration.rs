use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod stage {
    pub const PREPARING: &str = "Preparing";
    pub const ACTIVE: &str = "Active";
    pub const ROLLBACK_FAILED: &str = "RollbackFailed";
    pub const RESTORING: &str = "Restoring";
    pub const RESTORED: &str = "Restored";
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
}

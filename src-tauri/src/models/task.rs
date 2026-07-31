use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskKind {
    Scan,
    Migration,
    Restore,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDescriptor {
    pub id: Uuid,
    pub kind: TaskKind,
    pub state: TaskState,
    pub percentage: u8,
    pub phase: String,
    pub message: String,
    pub result_id: Option<Uuid>,
    pub started_at_utc: DateTime<Utc>,
    pub finished_at_utc: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TaskEvent {
    Progress {
        task: TaskDescriptor,
    },
    Completed {
        task: TaskDescriptor,
        summary: String,
    },
    Failed {
        task: TaskDescriptor,
        code: String,
        message: String,
        recoverable: bool,
    },
    Cancelled {
        task: TaskDescriptor,
    },
}

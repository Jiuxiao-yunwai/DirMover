//! Serializable domain models shared by commands and services.

pub mod migration;
pub mod scan;
pub mod settings;
pub mod system;
pub mod task;

pub use migration::{MoveRecord, MoveRow};
pub use scan::{
    FolderRow, LinkRecord, LinkRow, ScanPage, ScanProgress, ScanQuery, ScanSnapshot, SortDirection,
    SortField,
};
pub use settings::{
    AppSettings, AppSettingsDto, ManagedCategory, ManagedCategoryRow, MigrationPlan,
};
pub use system::{BootstrapData, DriveInfo, ScanPreset};
pub use task::{TaskDescriptor, TaskEvent, TaskKind, TaskState};

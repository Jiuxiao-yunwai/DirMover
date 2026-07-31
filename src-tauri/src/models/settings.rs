use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct AppSettings {
    #[serde(default)]
    pub managed_root: Option<String>,
    #[serde(default)]
    pub category_roots: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsDto {
    #[serde(default)]
    pub managed_root: Option<String>,
    #[serde(default)]
    pub category_roots: BTreeMap<String, String>,
}

impl From<AppSettings> for AppSettingsDto {
    fn from(settings: AppSettings) -> Self {
        Self {
            managed_root: settings.managed_root,
            category_roots: settings.category_roots,
        }
    }
}

impl From<AppSettingsDto> for AppSettings {
    fn from(settings: AppSettingsDto) -> Self {
        Self {
            managed_root: settings.managed_root,
            category_roots: settings.category_roots,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedCategory {
    pub key: &'static str,
    pub label: &'static str,
    pub source_root: Option<String>,
    pub default_relative_root: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedCategoryRow {
    pub key: String,
    pub label: String,
    pub path: Option<String>,
    pub overridden: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlan {
    pub source: String,
    pub category_key: String,
    pub category_label: String,
    pub managed_root: Option<String>,
    pub destination: Option<String>,
    pub available: bool,
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, AppSettingsDto};

    #[test]
    fn reads_and_writes_legacy_pascal_case() {
        let json = r#"{"ManagedRoot":"D:\\DirMoverData","CategoryRoots":{"Local":"E:\\Local"}}"#;
        let settings: AppSettings =
            serde_json::from_str(json).expect("legacy settings should parse");
        assert_eq!(settings.managed_root.as_deref(), Some(r"D:\DirMoverData"));
        let encoded = serde_json::to_string(&settings).expect("settings should serialize");
        assert!(encoded.contains("\"ManagedRoot\""));
        assert!(encoded.contains("\"CategoryRoots\""));
    }

    #[test]
    fn ipc_settings_use_camel_case() {
        let settings = AppSettingsDto {
            managed_root: Some(r"D:\DirMoverData".to_string()),
            category_roots: Default::default(),
        };
        let value = serde_json::to_value(settings).expect("IPC settings should serialize");
        assert!(value.get("managedRoot").is_some());
        assert!(value.get("ManagedRoot").is_none());
    }
}

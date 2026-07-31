use crate::{
    error::{AppError, AppResult},
    models::{AppSettings, ManagedCategory, ManagedCategoryRow},
    platform,
};
use std::path::{Component, Path, PathBuf};

pub const MANAGED_FOLDER_NAME: &str = "DirMoverData";

pub fn categories() -> Vec<ManagedCategory> {
    let profile = environment_path("USERPROFILE");
    vec![
        ManagedCategory {
            key: "Local",
            label: "AppData Local",
            source_root: environment_path("LOCALAPPDATA"),
            default_relative_root: r"AppData\Local",
        },
        ManagedCategory {
            key: "LocalLow",
            label: "AppData LocalLow",
            source_root: profile.as_ref().map(|path| {
                Path::new(path)
                    .join("AppData")
                    .join("LocalLow")
                    .to_string_lossy()
                    .into_owned()
            }),
            default_relative_root: r"AppData\LocalLow",
        },
        ManagedCategory {
            key: "Roaming",
            label: "AppData Roaming",
            source_root: environment_path("APPDATA"),
            default_relative_root: r"AppData\Roaming",
        },
        ManagedCategory {
            key: "ProgramFilesX86",
            label: "Program Files (x86)",
            source_root: environment_path("PROGRAMFILES(X86)"),
            default_relative_root: "Program Files (x86)",
        },
        ManagedCategory {
            key: "ProgramFiles",
            label: "Program Files",
            source_root: environment_path("PROGRAMFILES"),
            default_relative_root: "Program Files",
        },
        ManagedCategory {
            key: "ProgramData",
            label: "ProgramData",
            source_root: environment_path("PROGRAMDATA"),
            default_relative_root: "ProgramData",
        },
        ManagedCategory {
            key: "Other",
            label: "其他目录",
            source_root: None,
            default_relative_root: "Other",
        },
    ]
}

pub fn default_scan_root() -> String {
    environment_path("LOCALAPPDATA")
        .or_else(|| environment_path("USERPROFILE"))
        .unwrap_or_default()
}

pub fn automatic_managed_root() -> Option<PathBuf> {
    let drives = platform::fixed_ntfs_drives();
    let system_root = system_drive_root();
    let candidates = drives
        .into_iter()
        .filter(|drive| !path_text_equal(&drive.root, &system_root))
        .collect::<Vec<_>>();
    candidates
        .last()
        .map(|drive| Path::new(&drive.root).join(MANAGED_FOLDER_NAME))
}

pub fn category_for_source(source: &Path) -> ManagedCategory {
    let categories = categories();
    categories
        .iter()
        .find(|category| {
            category
                .source_root
                .as_ref()
                .is_some_and(|root| same_or_inside(source, Path::new(root)))
        })
        .cloned()
        .unwrap_or_else(|| {
            categories
                .last()
                .expect("the Other category is always present")
                .clone()
        })
}

pub fn category_root(settings: &AppSettings, category: &ManagedCategory) -> Option<PathBuf> {
    if let Some(overridden) = settings.category_roots.get(category.key)
        && !overridden.trim().is_empty()
    {
        return Some(PathBuf::from(overridden));
    }
    let base = settings
        .managed_root
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(automatic_managed_root)?;
    Some(base.join(category.default_relative_root))
}

pub fn managed_locations(settings: &AppSettings) -> Vec<ManagedCategoryRow> {
    categories()
        .into_iter()
        .map(|category| ManagedCategoryRow {
            key: category.key.to_string(),
            label: category.label.to_string(),
            path: category_root(settings, &category)
                .map(|path| path.to_string_lossy().into_owned()),
            overridden: settings.category_roots.contains_key(category.key),
        })
        .collect()
}

pub fn destination_under_base(base: &Path, source: &Path) -> AppResult<PathBuf> {
    let category = category_for_source(source);
    destination_under_category(
        &base.join(category.default_relative_root),
        source,
        &category,
    )
}

pub fn destination_under_category(
    category_root: &Path,
    source: &Path,
    category: &ManagedCategory,
) -> AppResult<PathBuf> {
    if let Some(source_root) = &category.source_root {
        let relative = relative_path_case_insensitive(source, Path::new(source_root))?;
        return Ok(category_root.join(relative));
    }

    let root = drive_root(source)?;
    let drive_label = root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace(':', "");
    let relative = relative_path_case_insensitive(source, &root)?;
    Ok(category_root.join(drive_label).join(relative))
}

pub fn validate_managed_root(root: &Path, source: Option<&Path>) -> AppResult<()> {
    let target_drive = platform::drive_info(root)?;
    if !target_drive.ready
        || !target_drive.fixed
        || !target_drive.file_system.eq_ignore_ascii_case("NTFS")
    {
        return Err(AppError::InvalidInput(
            "托管位置必须位于可用的固定 NTFS 分区。".to_string(),
        ));
    }
    if let Some(source) = source {
        let source_drive = platform::drive_info(source)?;
        if path_text_equal(&source_drive.root, &target_drive.root) {
            return Err(AppError::InvalidInput(
                "托管位置与源目录在同一分区，无法释放源磁盘空间。".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn same_or_inside(path: &Path, parent: &Path) -> bool {
    let path = normalized_components(path);
    let parent = normalized_components(parent);
    path.len() >= parent.len()
        && path
            .iter()
            .zip(parent.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

pub fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = normalized_components(left);
    let right = normalized_components(right);
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

fn normalized_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().into_owned()),
            Component::RootDir => Some("\\".to_string()),
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
        })
        .collect()
}

fn relative_path_case_insensitive(path: &Path, parent: &Path) -> AppResult<PathBuf> {
    let path_components = path.components().collect::<Vec<_>>();
    let parent_components = parent.components().collect::<Vec<_>>();
    if path_components.len() < parent_components.len()
        || !path_components
            .iter()
            .zip(parent_components.iter())
            .all(|(left, right)| {
                left.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
            })
    {
        return Err(AppError::InvalidInput(
            "源目录不属于所选托管分类。".to_string(),
        ));
    }
    Ok(path_components[parent_components.len()..]
        .iter()
        .map(|component| component.as_os_str())
        .collect())
}

fn drive_root(path: &Path) -> AppResult<PathBuf> {
    let value = path.to_string_lossy();
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Ok(PathBuf::from(format!(
            "{}:\\",
            char::from(bytes[0].to_ascii_uppercase())
        )));
    }
    Err(AppError::InvalidInput(
        "路径必须位于带盘符的本地磁盘。".to_string(),
    ))
}

fn system_drive_root() -> String {
    std::env::var("SystemDrive")
        .map(|value| format!("{}\\", value.trim_end_matches(['\\', '/'])))
        .unwrap_or_else(|_| "C:\\".to_string())
}

fn environment_path(name: &str) -> Option<String> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(value).to_string_lossy().into_owned())
}

fn path_text_equal(left: &str, right: &str) -> bool {
    left.trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(right.trim_end_matches(['\\', '/']))
}

#[cfg(test)]
mod tests {
    use super::{
        categories, category_for_source, destination_under_category, paths_equal, same_or_inside,
    };
    use std::path::Path;

    #[test]
    fn detects_windows_path_boundaries_case_insensitively() {
        assert!(same_or_inside(
            Path::new(r"C:\Users\A\AppData\Local\Vendor"),
            Path::new(r"c:\users\a\AppData\Local")
        ));
        assert!(!same_or_inside(
            Path::new(r"C:\Users\A\AppData\LocalElse"),
            Path::new(r"C:\Users\A\AppData\Local")
        ));
        assert!(paths_equal(Path::new(r"C:\One\"), Path::new(r"c:\one")));
    }

    #[test]
    fn preserves_legacy_category_destination_shape() {
        let source = Path::new(r"C:\Users\A\AppData\Local\Vendor");
        let category = category_for_source(source);
        if category.key == "Other" {
            // Non-Windows CI may not expose the same user environment.
            return;
        }
        let destination = destination_under_category(
            Path::new(r"Z:\DirMoverData\AppData\Local"),
            source,
            &category,
        )
        .expect("destination should be planned");
        assert!(destination.ends_with("Vendor"));
        assert!(categories().iter().any(|item| item.key == "Other"));
    }
}

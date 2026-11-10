//! Directory preview, filesystem fallback scanning, and NTFS MFT scanning.

mod enumeration;

use crate::{
    error::{AppError, AppResult},
    models::{FolderRow, LinkRow, ScanProgress, ScanSnapshot},
    platform,
    tasks::CancellationFlag,
};
use chrono::Utc;
use std::{
    fs,
    os::windows::fs::MetadataExt,
    path::{Component, Path},
    time::{Duration, Instant},
};
use uuid::Uuid;

const SMALL_TREE_MAX_ENTRIES: usize = 1024;
const SMALL_TREE_PROBE_TIME: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderStats {
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub files: u64,
    pub links: u64,
    pub errors: u64,
}

/// Migration preflight needs logical size and safety checks, not allocated size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MigrationStats {
    pub bytes: u64,
    pub files: u64,
    pub links: u64,
    pub errors: u64,
}

pub fn list_child_folders(
    root: &Path,
    cancellation: &CancellationFlag,
) -> AppResult<Vec<FolderRow>> {
    validate_directory(root)?;
    let mut rows = Vec::new();
    for entry in fs::read_dir(root)? {
        cancellation.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let is_link = platform::is_reparse_point_from_attributes(metadata.file_attributes());
        let is_directory = metadata.is_dir() || is_link;
        if !is_directory {
            continue;
        }
        let path = entry.path();
        let target = if is_link {
            platform::link_target(&path)
                .ok()
                .flatten()
                .map(|path| path.to_string_lossy().into_owned())
        } else {
            None
        };
        rows.push(FolderRow {
            name: entry.file_name().to_string_lossy().into_owned(),
            full_path: path.to_string_lossy().into_owned(),
            bytes: 0,
            allocated_bytes: 0,
            file_count: 0,
            link_count: u64::from(is_link),
            error_count: 0,
            is_link,
            link_target: target,
            note: if is_link {
                "目录链接".to_string()
            } else {
                "未扫描".to_string()
            },
            measured: false,
        });
    }
    rows.sort_by_cached_key(|row| row.name.to_lowercase());
    Ok(rows)
}

pub fn scan_children(
    root: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<ScanSnapshot> {
    scan_children_with_probe(root, cancellation, progress, SMALL_TREE_PROBE_TIME)
}

fn scan_children_with_probe(
    root: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
    probe_time: Duration,
) -> AppResult<ScanSnapshot> {
    validate_directory(root)?;
    cancellation.check()?;
    if has_small_tree(root, cancellation, SMALL_TREE_MAX_ENTRIES, probe_time)? {
        let mut snapshot = scan_children_by_enumeration(root, cancellation, progress)?;
        snapshot.engine = "快速遍历".to_string();
        return Ok(snapshot);
    }
    #[cfg(windows)]
    {
        match crate::platform::scan_mft_children(root, cancellation, progress) {
            Ok(mft) => {
                let links = mft
                    .links
                    .iter()
                    .map(|(path, is_directory)| create_link_row(path, *is_directory, root))
                    .collect::<Vec<_>>();
                let folders = mft
                    .folders
                    .into_iter()
                    .map(|folder| {
                        let path = root.join(&folder.name);
                        let target = if folder.is_reparse {
                            platform::link_target(&path)
                                .ok()
                                .flatten()
                                .map(|target| target.to_string_lossy().into_owned())
                        } else {
                            None
                        };
                        FolderRow {
                            name: folder.name,
                            full_path: path.to_string_lossy().into_owned(),
                            bytes: folder.logical_bytes,
                            allocated_bytes: folder.allocated_bytes,
                            file_count: folder.files,
                            link_count: u64::from(folder.is_reparse),
                            error_count: 0,
                            is_link: folder.is_reparse,
                            link_target: target,
                            note: if folder.is_reparse {
                                "MFT：目录链接".to_string()
                            } else {
                                "MFT 极速扫描".to_string()
                            },
                            measured: true,
                        }
                    })
                    .collect();
                return Ok(ScanSnapshot {
                    id: Uuid::new_v4(),
                    root: root.to_string_lossy().into_owned(),
                    folders,
                    links,
                    root_bytes: mft.root_logical_bytes,
                    root_allocated_bytes: mft.root_allocated_bytes,
                    root_files: mft.root_files,
                    engine: "NTFS MFT".to_string(),
                    created_at_utc: Utc::now(),
                });
            }
            Err(AppError::Cancelled) => return Err(AppError::Cancelled),
            Err(_) => {
                progress(ScanProgress {
                    percentage: None,
                    phase: "fallback".to_string(),
                    message: "MFT 扫描不可用，正在切换到兼容扫描…".to_string(),
                });
            }
        }
    }
    scan_children_by_enumeration(root, cancellation, progress)
}

pub fn scan_children_by_enumeration(
    root: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<ScanSnapshot> {
    enumeration::scan(root, cancellation, progress)
}

/// A bounded metadata-only probe avoids reading an entire volume's MFT for a
/// small subtree. It never queries allocated sizes or follows reparse points.
/// Large/erroring trees retain the existing MFT-then-enumeration path.
fn has_small_tree(
    root: &Path,
    cancellation: &CancellationFlag,
    max_entries: usize,
    time_limit: Duration,
) -> AppResult<bool> {
    if !root
        .components()
        .any(|part| matches!(part, Component::Normal(_)))
    {
        return Ok(false);
    }
    let started = Instant::now();
    let mut seen = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        cancellation.check()?;
        if started.elapsed() >= time_limit {
            return Ok(false);
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => return Ok(false),
        };
        for entry in entries {
            cancellation.check()?;
            if seen >= max_entries || started.elapsed() >= time_limit {
                return Ok(false);
            }
            seen += 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => return Ok(false),
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => return Ok(false),
            };
            let attributes = metadata.file_attributes();
            if (metadata.is_dir() || attributes & 0x10 != 0)
                && !platform::is_reparse_point_from_attributes(attributes)
            {
                pending.push(entry.path());
            }
        }
    }
    cancellation.check()?;
    Ok(started.elapsed() < time_limit)
}

/// Reuse the metadata returned by Windows directory enumeration. This avoids
/// opening every entry again and querying allocation information that migration
/// never consumes. Reparse points are still classified without following them;
/// Robocopy remains responsible for reporting file-content access failures.
pub fn measure_for_migration(root: &Path, cancellation: &CancellationFlag) -> MigrationStats {
    let stats = measure_with(root, cancellation, |entry| {
        let metadata = entry.metadata()?;
        Ok(classify_entry(&metadata, || 0))
    });
    MigrationStats {
        bytes: stats.bytes,
        files: stats.files,
        links: stats.links,
        errors: stats.errors,
    }
}

fn measure_with(
    root: &Path,
    cancellation: &CancellationFlag,
    inspect: impl Fn(&fs::DirEntry) -> AppResult<MeasureEntry>,
) -> FolderStats {
    let mut stats = FolderStats::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        if cancellation.is_cancelled() {
            break;
        }
        let entries = match fs::read_dir(current) {
            Ok(entries) => entries,
            Err(_) => {
                stats.errors += 1;
                continue;
            }
        };
        for entry in entries {
            if cancellation.is_cancelled() {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    stats.errors += 1;
                    continue;
                }
            };
            match inspect(&entry) {
                Ok(MeasureEntry::Link) => stats.links += 1,
                Ok(MeasureEntry::Directory) => stack.push(entry.path()),
                Ok(MeasureEntry::File {
                    bytes,
                    allocated_bytes,
                    is_link,
                }) => {
                    stats.bytes = stats.bytes.saturating_add(bytes);
                    stats.allocated_bytes = stats.allocated_bytes.saturating_add(allocated_bytes);
                    stats.files += 1;
                    stats.links = stats.links.saturating_add(u64::from(is_link));
                }
                Err(_) => stats.errors += 1,
            }
        }
    }
    stats
}

pub fn find_links(root: &Path, cancellation: &CancellationFlag) -> AppResult<Vec<LinkRow>> {
    validate_directory(root)?;
    let mut found = Vec::new();
    for entry in fs::read_dir(root)? {
        cancellation.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let attributes = metadata.file_attributes();
        if platform::is_reparse_point_from_attributes(attributes) {
            let is_directory = metadata.is_dir() || attributes & 0x10 != 0;
            found.push(create_link_row(&entry.path(), is_directory, root));
        }
    }
    Ok(found)
}

pub fn create_link_row(path: &Path, is_directory: bool, scan_root: &Path) -> LinkRow {
    let target = platform::link_target(path).ok().flatten();
    let resolved = target.as_ref().map(|target| {
        if target.is_absolute() {
            target.clone()
        } else {
            path.parent().unwrap_or(scan_root).join(target)
        }
    });
    let valid = resolved.as_ref().is_some_and(|target| target.exists());
    LinkRow {
        kind: if is_directory {
            "目录链接".to_string()
        } else {
            "文件链接".to_string()
        },
        path: path.to_string_lossy().into_owned(),
        target: target
            .map(|target| target.to_string_lossy().into_owned())
            .unwrap_or_else(|| "无法解析".to_string()),
        state: if valid { "有效" } else { "失效" }.to_string(),
        scan_root: scan_root.to_string_lossy().into_owned(),
        detected_at_utc: Some(Utc::now()),
    }
}

enum MeasureEntry {
    Link,
    Directory,
    File {
        bytes: u64,
        allocated_bytes: u64,
        is_link: bool,
    },
}

fn measure_cached_entry(entry: &fs::DirEntry) -> AppResult<MeasureEntry> {
    let metadata = entry.metadata()?;
    Ok(classify_entry(&metadata, || {
        platform::allocated_size(&entry.path()).unwrap_or(metadata.len())
    }))
}

fn classify_entry(metadata: &fs::Metadata, allocated_bytes: impl FnOnce() -> u64) -> MeasureEntry {
    let attributes = metadata.file_attributes();
    let is_reparse = platform::is_reparse_point_from_attributes(attributes);
    let is_directory = metadata.is_dir() || attributes & 0x10 != 0;
    if is_directory {
        if is_reparse {
            MeasureEntry::Link
        } else {
            MeasureEntry::Directory
        }
    } else {
        MeasureEntry::File {
            bytes: metadata.len(),
            allocated_bytes: allocated_bytes(),
            is_link: is_reparse,
        }
    }
}

fn validate_directory(root: &Path) -> AppResult<()> {
    if root.as_os_str().is_empty() {
        return Err(AppError::InvalidInput("目录路径不能为空。".to_string()));
    }
    if !root.exists() {
        return Err(AppError::NotFound("所选目录不存在。".to_string()));
    }
    if !root.is_dir() {
        return Err(AppError::InvalidInput("所选路径不是一个目录。".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

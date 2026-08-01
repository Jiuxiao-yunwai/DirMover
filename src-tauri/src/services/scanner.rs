//! Directory preview, filesystem fallback scanning, and NTFS MFT scanning.

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
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderStats {
    pub bytes: u64,
    pub allocated_bytes: u64,
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
        let path = entry.path();
        let is_link = platform::is_reparse_point(&path).unwrap_or(false);
        let is_directory = entry
            .file_type()
            .map(|file_type| file_type.is_dir())
            .unwrap_or(false)
            || is_link;
        if !is_directory {
            continue;
        }
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
    rows.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    Ok(rows)
}

pub fn scan_children(
    root: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<ScanSnapshot> {
    validate_directory(root)?;
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
                    engine: "NTFS MFT".to_string(),
                    created_at_utc: Utc::now(),
                });
            }
            Err(AppError::Cancelled) => return Err(AppError::Cancelled),
            Err(_) => {
                progress(ScanProgress {
                    percentage: 2,
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
    let directories = list_child_paths(root)?;
    let total = directories.len();
    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let rows = Mutex::new(Vec::with_capacity(total));
    let links = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .clamp(2, 8)
        .min(total.max(1));

    progress(ScanProgress {
        percentage: 5,
        phase: "fallback".to_string(),
        message: "正在使用兼容扫描…".to_string(),
    });

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    if cancellation.is_cancelled() {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = directories.get(index) else {
                        break;
                    };
                    let row = scan_top_folder(path, root, cancellation, &links);
                    if let Ok(mut target) = rows.lock() {
                        target.push(row);
                    }
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    let percentage = if total == 0 {
                        100
                    } else {
                        5 + (done * 94 / total).min(94)
                    };
                    progress(ScanProgress {
                        percentage: u8::try_from(percentage).unwrap_or(99),
                        phase: "fallback".to_string(),
                        message: format!("正在扫描文件夹 {done}/{total}"),
                    });
                }
            });
        }
    });
    cancellation.check()?;

    let rows = rows
        .into_inner()
        .map_err(|_| AppError::Task("扫描结果锁已损坏。".to_string()))?;
    let links = links
        .into_inner()
        .map_err(|_| AppError::Task("链接结果锁已损坏。".to_string()))?;
    progress(ScanProgress {
        percentage: 100,
        phase: "complete".to_string(),
        message: "扫描完成".to_string(),
    });
    Ok(ScanSnapshot {
        id: Uuid::new_v4(),
        root: root.to_string_lossy().into_owned(),
        folders: rows,
        links,
        engine: "兼容模式".to_string(),
        created_at_utc: Utc::now(),
    })
}

pub fn measure(root: &Path, cancellation: &CancellationFlag) -> FolderStats {
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
            let path = entry.path();
            match measure_entry(&path) {
                Ok(MeasureEntry::Link) => stats.links += 1,
                Ok(MeasureEntry::Directory) => stack.push(path),
                Ok(MeasureEntry::File(bytes)) => {
                    stats.bytes = stats.bytes.saturating_add(bytes);
                    stats.allocated_bytes = stats.allocated_bytes.saturating_add(bytes);
                    stats.files += 1;
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
        let path = entry.path();
        if platform::is_reparse_point(&path).unwrap_or(false) {
            let is_directory = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            found.push(create_link_row(&path, is_directory, root));
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

fn list_child_paths(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
            || platform::is_reparse_point(&path).unwrap_or(false)
        {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn scan_top_folder(
    path: &Path,
    scan_root: &Path,
    cancellation: &CancellationFlag,
    links: &Mutex<Vec<LinkRow>>,
) -> FolderRow {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_link = platform::is_reparse_point(path).unwrap_or(false);
    if is_link {
        let link = create_link_row(path, true, scan_root);
        if let Ok(mut target) = links.lock() {
            target.push(link.clone());
        }
        return FolderRow {
            name,
            full_path: path.to_string_lossy().into_owned(),
            bytes: 0,
            allocated_bytes: 0,
            file_count: 0,
            link_count: 1,
            error_count: 0,
            is_link: true,
            link_target: (link.target != "无法解析").then_some(link.target),
            note: "兼容扫描：目录链接".to_string(),
            measured: true,
        };
    }

    let stats = measure(path, cancellation);
    FolderRow {
        name,
        full_path: path.to_string_lossy().into_owned(),
        bytes: stats.bytes,
        allocated_bytes: stats.allocated_bytes,
        file_count: stats.files,
        link_count: stats.links,
        error_count: stats.errors,
        is_link: false,
        link_target: None,
        note: if stats.errors == 0 {
            "兼容扫描".to_string()
        } else {
            format!("兼容扫描：{} 项无法读取", stats.errors)
        },
        measured: true,
    }
}

enum MeasureEntry {
    Link,
    Directory,
    File(u64),
}

fn measure_entry(path: &Path) -> AppResult<MeasureEntry> {
    let metadata = fs::symlink_metadata(path)?;
    if platform::is_reparse_point_from_attributes(metadata.file_attributes()) {
        return Ok(MeasureEntry::Link);
    }
    if metadata.is_dir() {
        Ok(MeasureEntry::Directory)
    } else {
        Ok(MeasureEntry::File(metadata.len()))
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

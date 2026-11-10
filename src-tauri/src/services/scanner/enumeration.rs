//! Directory-granularity scheduling keeps large single-root trees parallel.

use super::{FolderStats, MeasureEntry, create_link_row, measure_cached_entry};
use crate::{
    error::{AppError, AppResult},
    models::{FolderRow, LinkRow, ScanProgress, ScanSnapshot},
    tasks::CancellationFlag,
};
use chrono::Utc;
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

struct DirectoryJob {
    path: PathBuf,
    owner: usize,
}

struct RootEntries {
    folders: Vec<FolderRow>,
    links: Vec<LinkRow>,
    stats: FolderStats,
    jobs: VecDeque<DirectoryJob>,
}

struct QueueState {
    jobs: VecDeque<DirectoryJob>,
    active: usize,
    stats: Vec<FolderStats>,
    discovered: usize,
    completed: usize,
    last_progress: Instant,
}

pub(super) fn scan(
    root: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<ScanSnapshot> {
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .clamp(2, 8);
    scan_with_workers(root, cancellation, progress, workers)
}

fn scan_with_workers(
    root: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
    workers: usize,
) -> AppResult<ScanSnapshot> {
    cancellation.check()?;
    progress(ScanProgress {
        percentage: None,
        phase: "enumerate".to_string(),
        message: "正在读取目录信息…".to_string(),
    });
    // Read the root once: collect both immediate files and child folders.
    let mut entries = read_root(root, cancellation)?;
    let queue = Mutex::new(QueueState {
        discovered: entries.jobs.len(),
        jobs: std::mem::take(&mut entries.jobs),
        active: 0,
        stats: vec![FolderStats::default(); entries.folders.len()],
        completed: 0,
        last_progress: Instant::now(),
    });
    let ready = Condvar::new();
    let has_jobs = queue.lock().map_err(queue_error)?.discovered > 0;
    if has_jobs {
        std::thread::scope(|scope| -> AppResult<()> {
            let handles = (0..workers.max(1))
                .map(|_| scope.spawn(|| worker(&queue, &ready, cancellation, progress)))
                .collect::<Vec<_>>();
            for handle in handles {
                handle
                    .join()
                    .map_err(|_| AppError::Task("目录扫描线程异常结束。".to_string()))??;
            }
            Ok(())
        })?;
    }
    cancellation.check()?;
    let state = queue.into_inner().map_err(queue_error)?;
    for (row, stats) in entries.folders.iter_mut().zip(state.stats) {
        if row.is_link {
            continue;
        }
        row.bytes = stats.bytes;
        row.allocated_bytes = stats.allocated_bytes;
        row.file_count = stats.files;
        row.link_count = stats.links;
        row.error_count = stats.errors;
        if stats.errors > 0 {
            row.note = format!("兼容扫描：{} 项无法读取", stats.errors);
        }
    }
    progress(ScanProgress {
        percentage: Some(100),
        phase: "complete".to_string(),
        message: "扫描完成".to_string(),
    });
    Ok(ScanSnapshot {
        id: Uuid::new_v4(),
        root: root.to_string_lossy().into_owned(),
        folders: entries.folders,
        links: entries.links,
        root_bytes: entries.stats.bytes,
        root_allocated_bytes: entries.stats.allocated_bytes,
        root_files: entries.stats.files,
        engine: "兼容模式".to_string(),
        created_at_utc: Utc::now(),
    })
}

fn worker(
    queue: &Mutex<QueueState>,
    ready: &Condvar,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<()> {
    loop {
        let job = {
            let mut state = queue.lock().map_err(queue_error)?;
            loop {
                if cancellation.is_cancelled() {
                    ready.notify_all();
                    return Ok(());
                }
                if let Some(job) = state.jobs.pop_front() {
                    state.active += 1;
                    break job;
                }
                if state.active == 0 {
                    return Ok(());
                }
                // Cancellation does not itself notify this condition variable.
                state = ready
                    .wait_timeout(state, Duration::from_millis(100))
                    .map_err(queue_error)?
                    .0;
            }
        };
        let (stats, children) = read_directory(&job, cancellation);
        let update = {
            let mut state = queue.lock().map_err(queue_error)?;
            merge_stats(&mut state.stats[job.owner], stats);
            state.discovered += children.len();
            state.jobs.extend(children);
            state.completed += 1;
            state.active -= 1;
            ready.notify_all();
            if state.last_progress.elapsed() >= Duration::from_millis(80) {
                state.last_progress = Instant::now();
                Some(ScanProgress {
                    percentage: None,
                    phase: "fallback".to_string(),
                    message: format!("已扫描 {} / {} 个目录", state.completed, state.discovered),
                })
            } else {
                None
            }
        };
        if let Some(update) = update {
            progress(update);
        }
    }
}

fn read_root(root: &Path, cancellation: &CancellationFlag) -> AppResult<RootEntries> {
    let mut result = RootEntries {
        folders: Vec::new(),
        links: Vec::new(),
        stats: FolderStats::default(),
        jobs: VecDeque::new(),
    };
    for entry in fs::read_dir(root)? {
        cancellation.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                result.stats.errors += 1;
                continue;
            }
        };
        match measure_cached_entry(&entry) {
            Ok(item @ (MeasureEntry::Directory | MeasureEntry::Link)) => {
                let is_link = matches!(item, MeasureEntry::Link);
                let path = entry.path();
                let link = is_link.then(|| create_link_row(&path, true, root));
                let link_target = link
                    .as_ref()
                    .and_then(|link| (link.target != "无法解析").then(|| link.target.clone()));
                if let Some(link) = link {
                    result.links.push(link);
                } else {
                    result.jobs.push_back(DirectoryJob {
                        path: path.clone(),
                        owner: result.folders.len(),
                    });
                }
                result.folders.push(FolderRow {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    full_path: path.to_string_lossy().into_owned(),
                    bytes: 0,
                    allocated_bytes: 0,
                    file_count: 0,
                    link_count: u64::from(is_link),
                    error_count: 0,
                    is_link,
                    link_target,
                    note: if is_link {
                        "兼容扫描：目录链接"
                    } else {
                        "兼容扫描"
                    }
                    .to_string(),
                    measured: true,
                });
            }
            Ok(item) => add_entry_stats(&mut result.stats, item),
            Err(_) => result.stats.errors += 1,
        }
    }
    Ok(result)
}

fn read_directory(
    job: &DirectoryJob,
    cancellation: &CancellationFlag,
) -> (FolderStats, Vec<DirectoryJob>) {
    let mut stats = FolderStats::default();
    let mut children = Vec::new();
    let entries = match fs::read_dir(&job.path) {
        Ok(entries) => entries,
        Err(_) => {
            stats.errors = 1;
            return (stats, children);
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
        match measure_cached_entry(&entry) {
            Ok(MeasureEntry::Directory) => children.push(DirectoryJob {
                path: entry.path(),
                owner: job.owner,
            }),
            Ok(item) => add_entry_stats(&mut stats, item),
            Err(_) => stats.errors += 1,
        }
    }
    (stats, children)
}

fn add_entry_stats(stats: &mut FolderStats, item: MeasureEntry) {
    match item {
        MeasureEntry::File {
            bytes,
            allocated_bytes,
            is_link,
        } => {
            stats.bytes = stats.bytes.saturating_add(bytes);
            stats.allocated_bytes = stats.allocated_bytes.saturating_add(allocated_bytes);
            stats.files = stats.files.saturating_add(1);
            stats.links = stats.links.saturating_add(u64::from(is_link));
        }
        MeasureEntry::Link => stats.links = stats.links.saturating_add(1),
        MeasureEntry::Directory => {}
    }
}

fn merge_stats(target: &mut FolderStats, source: FolderStats) {
    target.bytes = target.bytes.saturating_add(source.bytes);
    target.allocated_bytes = target
        .allocated_bytes
        .saturating_add(source.allocated_bytes);
    target.files = target.files.saturating_add(source.files);
    target.links = target.links.saturating_add(source.links);
    target.errors = target.errors.saturating_add(source.errors);
}

fn queue_error<T>(_: T) -> AppError {
    AppError::Task("扫描队列锁已损坏。".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_child_directory_is_counted_as_an_error() {
        let path = std::env::temp_dir().join(format!("dirmover-missing-{}", Uuid::new_v4()));
        let (stats, children) =
            read_directory(&DirectoryJob { path, owner: 0 }, &CancellationFlag::new());
        assert_eq!(stats.errors, 1);
        assert_eq!(stats.files, 0);
        assert!(children.is_empty());
    }

    #[test]
    fn cancellation_wakes_workers_waiting_for_new_directories() {
        let queue = Mutex::new(QueueState {
            jobs: VecDeque::new(),
            active: 1,
            stats: Vec::new(),
            discovered: 1,
            completed: 0,
            last_progress: Instant::now(),
        });
        let ready = Condvar::new();
        let cancellation = CancellationFlag::new();
        let (sent, received) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let result = worker(&queue, &ready, &cancellation, &|_| {});
                sent.send(result).unwrap();
            });
            cancellation.cancel();
            let outcome = received.recv_timeout(Duration::from_secs(3));
            // Ensure the test also terminates if cancellation handling regresses.
            queue.lock().unwrap().active = 0;
            ready.notify_all();
            outcome
                .expect("waiting worker must observe cancellation")
                .unwrap();
        });
    }
}

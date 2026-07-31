use crate::{
    error::{AppError, AppResult},
    models::{
        FolderRow, LinkRow, ScanPage, ScanProgress, ScanQuery, SortDirection, SortField,
        TaskDescriptor, TaskEvent, TaskKind,
    },
    services::scanner,
    state::AppState,
    tasks::CancellationFlag,
};
use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{State, ipc::Channel};
use uuid::Uuid;

#[tauri::command]
pub async fn list_child_folders(path: String) -> AppResult<Vec<FolderRow>> {
    tauri::async_runtime::spawn_blocking(move || {
        scanner::list_child_folders(Path::new(&path), &CancellationFlag::new())
    })
    .await
    .map_err(|error| AppError::Task(format!("目录预览任务异常结束：{error}")))?
}

#[tauri::command]
pub fn start_scan(
    root: String,
    force: bool,
    on_event: Channel<TaskEvent>,
    state: State<'_, AppState>,
) -> AppResult<TaskDescriptor> {
    let normalized = PathBuf::from(&root).to_string_lossy().into_owned();
    if !force && let Some(snapshot) = state.find_scan_by_root(&normalized)? {
        let (task, _) = state
            .tasks()
            .start(TaskKind::Scan, "cache", "正在读取扫描缓存…")?;
        let completed =
            state
                .tasks()
                .complete(task.id, Some(snapshot.id), "已加载本次运行的扫描缓存。")?;
        let _ = on_event.send(TaskEvent::Completed {
            task: completed.clone(),
            summary: "已加载本次运行的扫描缓存。".to_string(),
        });
        return Ok(completed);
    }

    let (task, cancellation) = state
        .tasks()
        .start(TaskKind::Scan, "prepare", "正在准备扫描…")?;
    let task_id = task.id;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let last_emitted = Arc::new(Mutex::new((
            u8::MAX,
            Instant::now() - Duration::from_secs(1),
        )));
        let event_state = state.clone();
        let event_channel = on_event.clone();
        let event_throttle = last_emitted.clone();
        let progress = move |update: ScanProgress| {
            let should_emit = event_throttle
                .lock()
                .map(|mut last| {
                    let now = Instant::now();
                    let changed = last.0 != update.percentage;
                    let elapsed = now.duration_since(last.1) >= Duration::from_millis(80);
                    if changed && elapsed {
                        *last = (update.percentage, now);
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if should_emit
                && let Ok(task) = event_state.tasks().update(
                    task_id,
                    update.percentage,
                    update.phase,
                    update.message,
                )
            {
                let _ = event_channel.send(TaskEvent::Progress { task });
            }
        };

        match scanner::scan_children(Path::new(&root), &cancellation, &progress) {
            Ok(snapshot) => {
                let summary = format!(
                    "扫描完成（{}）：{} 个一级文件夹",
                    snapshot.engine,
                    snapshot.folders.len()
                );
                let result_id = snapshot.id;
                let store_result = state
                    .storage()
                    .replace_links_for_root(Path::new(&root), &snapshot.links)
                    .and_then(|()| state.insert_scan(snapshot));
                match store_result {
                    Ok(_) => {
                        if let Ok(task) = state.tasks().complete(task_id, Some(result_id), &summary)
                        {
                            let _ = on_event.send(TaskEvent::Completed { task, summary });
                        }
                    }
                    Err(error) => emit_failure(&state, task_id, &on_event, error),
                }
            }
            Err(AppError::Cancelled) => {
                if let Ok(task) = state.tasks().cancelled(task_id) {
                    let _ = on_event.send(TaskEvent::Cancelled { task });
                }
            }
            Err(error) => emit_failure(&state, task_id, &on_event, error),
        }
    });
    Ok(task)
}

#[tauri::command]
pub fn cancel_task(task_id: Uuid, state: State<'_, AppState>) -> AppResult<TaskDescriptor> {
    state.tasks().cancel(task_id)
}

#[tauri::command]
pub fn get_task(task_id: Uuid, state: State<'_, AppState>) -> AppResult<TaskDescriptor> {
    state.tasks().get(task_id)
}

#[tauri::command]
pub fn query_scan_result(
    result_id: Uuid,
    query: ScanQuery,
    state: State<'_, AppState>,
) -> AppResult<ScanPage> {
    let snapshot = state.scan(result_id)?;
    let page_size = query.page_size.clamp(20, 500);
    let page = query.page.max(1);
    let mut items = snapshot.folders.clone();
    items.sort_by(|left, right| compare_folders(left, right, query.sort_field));
    if matches!(query.sort_direction, SortDirection::Descending) {
        items.reverse();
    }
    let total_items = items.len();
    let start = page.saturating_sub(1).saturating_mul(page_size);
    let visible = if start >= total_items {
        Vec::new()
    } else {
        items
            .into_iter()
            .skip(start)
            .take(page_size)
            .collect::<Vec<_>>()
    };
    Ok(ScanPage {
        result_id,
        root: snapshot.root,
        engine: snapshot.engine,
        created_at_utc: snapshot.created_at_utc,
        items: visible,
        total_items,
        page,
        page_size,
        total_bytes: snapshot.folders.iter().map(|item| item.bytes).sum(),
        total_allocated_bytes: snapshot
            .folders
            .iter()
            .map(|item| item.allocated_bytes)
            .sum(),
        total_files: snapshot.folders.iter().map(|item| item.file_count).sum(),
    })
}

#[tauri::command]
pub async fn detect_links(root: String, state: State<'_, AppState>) -> AppResult<Vec<LinkRow>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let links = scanner::find_links(Path::new(&root), &CancellationFlag::new())?;
        state
            .storage()
            .replace_links_for_root(Path::new(&root), &links)?;
        Ok(links)
    })
    .await
    .map_err(|error| AppError::Task(format!("链接检测任务异常结束：{error}")))?
}

fn compare_folders(left: &FolderRow, right: &FolderRow, field: SortField) -> Ordering {
    match field {
        SortField::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        SortField::Bytes => left.bytes.cmp(&right.bytes),
        SortField::AllocatedBytes => left.allocated_bytes.cmp(&right.allocated_bytes),
        SortField::FileCount => left.file_count.cmp(&right.file_count),
        SortField::Status => left.note.cmp(&right.note),
    }
}

fn emit_failure(state: &AppState, task_id: Uuid, channel: &Channel<TaskEvent>, error: AppError) {
    if let Ok(task) = state.tasks().fail(task_id, error.to_string()) {
        let _ = channel.send(TaskEvent::Failed {
            task,
            code: error.code().to_string(),
            message: error.to_string(),
            recoverable: error.recoverable(),
        });
    }
}

use crate::{
    error::{AppError, AppResult},
    models::{MoveRow, ScanProgress, TaskDescriptor, TaskEvent, TaskKind},
    services::migration,
    state::AppState,
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{State, ipc::Channel};
use uuid::Uuid;

#[tauri::command]
pub fn start_migration(
    source: String,
    destination: String,
    on_event: Channel<TaskEvent>,
    state: State<'_, AppState>,
) -> AppResult<TaskDescriptor> {
    let (task, cancellation) =
        state
            .tasks()
            .start(TaskKind::Migration, "prepare", "正在准备迁移…")?;
    let task_id = task.id;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let progress = progress_emitter(state.clone(), task_id, on_event.clone());
        match migration::move_directory(
            state.storage(),
            Path::new(&source),
            Path::new(&destination),
            &cancellation,
            &progress,
        ) {
            Ok(outcome) => {
                let _ = state.clear_scans();
                if let Ok(task) =
                    state
                        .tasks()
                        .complete(task_id, outcome.record_id, &outcome.summary)
                {
                    let _ = on_event.send(TaskEvent::Completed {
                        task,
                        summary: outcome.summary,
                    });
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
pub fn start_restore(
    record_id: Uuid,
    on_event: Channel<TaskEvent>,
    state: State<'_, AppState>,
) -> AppResult<TaskDescriptor> {
    let (task, cancellation) =
        state
            .tasks()
            .start(TaskKind::Restore, "prepare", "正在准备还原…")?;
    let task_id = task.id;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let progress = progress_emitter(state.clone(), task_id, on_event.clone());
        match migration::restore_directory(state.storage(), record_id, &cancellation, &progress) {
            Ok(outcome) => {
                let _ = state.clear_scans();
                if let Ok(task) =
                    state
                        .tasks()
                        .complete(task_id, outcome.record_id, &outcome.summary)
                {
                    let _ = on_event.send(TaskEvent::Completed {
                        task,
                        summary: outcome.summary,
                    });
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
pub async fn recover_move(record_id: Uuid, state: State<'_, AppState>) -> AppResult<String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let message = migration::recover_interrupted(state.storage(), record_id)?;
        state.clear_scans()?;
        Ok(message)
    })
    .await
    .map_err(|error| AppError::Task(format!("恢复任务异常结束：{error}")))?
}

#[tauri::command]
pub async fn list_moves(state: State<'_, AppState>) -> AppResult<Vec<MoveRow>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || migration::move_rows(state.storage()))
        .await
        .map_err(|error| AppError::Task(format!("迁移记录检查异常结束：{error}")))?
}

fn progress_emitter(
    state: AppState,
    task_id: Uuid,
    channel: Channel<TaskEvent>,
) -> impl Fn(ScanProgress) + Send + Sync {
    let last = Arc::new(Mutex::new((
        u8::MAX,
        Instant::now() - Duration::from_secs(1),
    )));
    move |update: ScanProgress| {
        let should_emit = last
            .lock()
            .map(|mut previous| {
                let now = Instant::now();
                let changed = previous.0 != update.percentage;
                let elapsed = now.duration_since(previous.1) >= Duration::from_millis(80);
                if changed && elapsed {
                    *previous = (update.percentage, now);
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if should_emit
            && let Ok(task) =
                state
                    .tasks()
                    .update(task_id, update.percentage, update.phase, update.message)
        {
            let _ = channel.send(TaskEvent::Progress { task });
        }
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

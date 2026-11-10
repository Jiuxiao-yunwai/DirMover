use crate::{
    error::{AppError, AppResult},
    models::{
        MigrationRequest, MoveRow, ScanProgress, TaskDescriptor, TaskEvent, TaskKind, TaskState,
    },
    services::migration,
    state::AppState,
    tasks::ProgressThrottle,
};
use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
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
    enqueue_migration(source, destination, on_event, state.inner())
}

#[tauri::command]
pub fn start_migrations(
    requests: Vec<MigrationRequest>,
    on_event: Channel<TaskEvent>,
    state: State<'_, AppState>,
) -> AppResult<Vec<TaskDescriptor>> {
    if requests.is_empty() {
        return Err(AppError::InvalidInput(
            "请至少选择一个待迁移文件夹。".to_string(),
        ));
    }
    let mut sources = HashSet::new();
    let mut destinations = HashSet::new();
    for request in &requests {
        if request.source.trim().is_empty() || request.destination.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "迁移来源和目标不能为空。".to_string(),
            ));
        }
        if !sources.insert(request.source.to_lowercase()) {
            return Err(AppError::InvalidInput(format!(
                "同一批次中不能重复迁移：{}",
                request.source
            )));
        }
        if !destinations.insert(request.destination.to_lowercase()) {
            return Err(AppError::InvalidInput(format!(
                "同一批次中存在重复目标：{}",
                request.destination
            )));
        }
    }

    requests
        .into_iter()
        .map(|request| {
            enqueue_migration(
                request.source,
                request.destination,
                on_event.clone(),
                state.inner(),
            )
        })
        .collect()
}

fn enqueue_migration(
    source: String,
    destination: String,
    on_event: Channel<TaskEvent>,
    state: &AppState,
) -> AppResult<TaskDescriptor> {
    let (task, cancellation) =
        state
            .tasks()
            .start_queued(TaskKind::Migration, "queued", "迁移任务已加入后台队列。")?;
    let task_id = task.id;
    let queue = state.migration_queue();
    let (ticket, ahead) = match queue.reserve() {
        Ok(reservation) => reservation,
        Err(error) => {
            let _ = state.tasks().fail(task_id, error.to_string());
            return Err(error);
        }
    };
    let task = if ahead > 0 {
        state.tasks().update_queued(
            task_id,
            "queued",
            format!("已加入后台队列，前方有 {ahead} 个任务。"),
        )?
    } else {
        task
    };
    let state = state.clone();
    tauri::async_runtime::spawn(async move {
        let turn = match queue.wait_turn(ticket).await {
            Ok(guard) => guard,
            Err(error) => {
                emit_failure(&state, task_id, &on_event, error);
                return;
            }
        };
        let failure_state = state.clone();
        let failure_channel = on_event.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _turn = turn;
            if cancellation.is_cancelled() {
                emit_cancelled(&state, task_id, &on_event);
                return;
            }
            match state
                .tasks()
                .mark_running(task_id, "prepare", "正在准备迁移…")
            {
                Ok(task) if task.state == TaskState::Running => {
                    let _ = on_event.send(TaskEvent::Progress { task });
                }
                Ok(_) => {
                    emit_cancelled(&state, task_id, &on_event);
                    return;
                }
                Err(error) => {
                    emit_failure(&state, task_id, &on_event, error);
                    return;
                }
            }
            let progress = progress_emitter(state.clone(), task_id, on_event.clone());
            let result = migration::move_directory(
                state.storage(),
                Path::new(&source),
                Path::new(&destination),
                &cancellation,
                &progress,
            );
            let _ = state.clear_scans();
            match result {
                Ok(outcome) => {
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
                    emit_cancelled(&state, task_id, &on_event);
                }
                Err(error) => emit_failure(&state, task_id, &on_event, error),
            }
        })
        .await;
        if let Err(error) = result {
            let _ = failure_state.clear_scans();
            emit_failure(
                &failure_state,
                task_id,
                &failure_channel,
                AppError::Task(format!("迁移工作线程异常结束：{error}")),
            );
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
            .start_queued(TaskKind::Restore, "queued", "还原任务已加入后台队列。")?;
    let task_id = task.id;
    let queue = state.migration_queue();
    let (ticket, ahead) = match queue.reserve() {
        Ok(reservation) => reservation,
        Err(error) => {
            let _ = state.tasks().fail(task_id, error.to_string());
            return Err(error);
        }
    };
    let task = if ahead > 0 {
        state.tasks().update_queued(
            task_id,
            "queued",
            format!("已加入后台队列，前方有 {ahead} 个任务。"),
        )?
    } else {
        task
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let turn = match queue.wait_turn(ticket).await {
            Ok(guard) => guard,
            Err(error) => {
                emit_failure(&state, task_id, &on_event, error);
                return;
            }
        };
        let failure_state = state.clone();
        let failure_channel = on_event.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _turn = turn;
            if cancellation.is_cancelled() {
                emit_cancelled(&state, task_id, &on_event);
                return;
            }
            match state
                .tasks()
                .mark_running(task_id, "prepare", "正在准备还原…")
            {
                Ok(task) if task.state == TaskState::Running => {
                    let _ = on_event.send(TaskEvent::Progress { task });
                }
                Ok(_) => {
                    emit_cancelled(&state, task_id, &on_event);
                    return;
                }
                Err(error) => {
                    emit_failure(&state, task_id, &on_event, error);
                    return;
                }
            }
            let progress = progress_emitter(state.clone(), task_id, on_event.clone());
            let result =
                migration::restore_directory(state.storage(), record_id, &cancellation, &progress);
            let _ = state.clear_scans();
            match result {
                Ok(outcome) => {
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
                    emit_cancelled(&state, task_id, &on_event);
                }
                Err(error) => emit_failure(&state, task_id, &on_event, error),
            }
        })
        .await;
        if let Err(error) = result {
            let _ = failure_state.clear_scans();
            emit_failure(
                &failure_state,
                task_id,
                &failure_channel,
                AppError::Task(format!("迁移工作线程异常结束：{error}")),
            );
        }
    });
    Ok(task)
}

#[tauri::command]
pub async fn recover_move(record_id: Uuid, state: State<'_, AppState>) -> AppResult<String> {
    let queue = state.migration_queue();
    let (ticket, _) = queue.reserve()?;
    let state = state.inner().clone();
    let turn = queue.wait_turn(ticket).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = turn;
        let result = migration::recover_interrupted(state.storage(), record_id);
        state.clear_scans()?;
        result
    })
    .await
    .map_err(|error| AppError::Task(format!("恢复任务异常结束：{error}")))?
}

fn emit_cancelled(state: &AppState, task_id: Uuid, channel: &Channel<TaskEvent>) {
    if let Ok(task) = state.tasks().cancelled(task_id) {
        let _ = channel.send(TaskEvent::Cancelled { task });
    }
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
    let last = Arc::new(Mutex::new(ProgressThrottle::default()));
    move |update: ScanProgress| {
        let should_emit = last
            .lock()
            .map(|mut previous| previous.allows(&update, Instant::now()))
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

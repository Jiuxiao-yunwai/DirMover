use crate::{
    error::{AppError, AppResult},
    models::{TaskDescriptor, TaskKind, TaskState},
};
use chrono::Utc;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU8, Ordering},
    },
};
use uuid::Uuid;

const MAX_TASK_HISTORY: usize = 64;

#[derive(Clone)]
pub struct CancellationFlag(Arc<AtomicU8>);

const AVAILABLE: u8 = 0;
const REQUESTED: u8 = 1;
const COMMITTED: u8 = 2;

impl CancellationFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU8::new(AVAILABLE)))
    }

    pub fn cancel(&self) -> bool {
        self.0
            .compare_exchange(AVAILABLE, REQUESTED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire) == REQUESTED
    }

    /// Cancellation and the irreversible cutover compete for the same state.
    pub fn begin_commit(&self) -> AppResult<()> {
        match self
            .0
            .compare_exchange(AVAILABLE, COMMITTED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) | Err(COMMITTED) => Ok(()),
            Err(_) => Err(AppError::Cancelled),
        }
    }

    pub fn check(&self) -> AppResult<()> {
        if self.is_cancelled() {
            Err(AppError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
pub struct TaskRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    entries: HashMap<Uuid, TaskEntry>,
    finished: VecDeque<Uuid>,
}

struct TaskEntry {
    descriptor: TaskDescriptor,
    cancellation: CancellationFlag,
}

impl TaskEntry {
    fn snapshot(&self) -> TaskDescriptor {
        let mut descriptor = self.descriptor.clone();
        let cancellation = self.cancellation.0.load(Ordering::Acquire);
        descriptor.can_cancel = matches!(descriptor.state, TaskState::Queued | TaskState::Running)
            && cancellation == AVAILABLE;
        descriptor.cancel_requested = cancellation == REQUESTED;
        descriptor
    }
}

impl TaskRegistry {
    pub fn start(
        &self,
        kind: TaskKind,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> AppResult<(TaskDescriptor, CancellationFlag)> {
        self.start_with_state(kind, TaskState::Running, phase, message)
    }

    pub fn start_queued(
        &self,
        kind: TaskKind,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> AppResult<(TaskDescriptor, CancellationFlag)> {
        self.start_with_state(kind, TaskState::Queued, phase, message)
    }

    fn start_with_state(
        &self,
        kind: TaskKind,
        state: TaskState,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> AppResult<(TaskDescriptor, CancellationFlag)> {
        let descriptor = TaskDescriptor {
            id: Uuid::new_v4(),
            kind,
            state,
            can_cancel: true,
            cancel_requested: false,
            percentage: None,
            phase: phase.into(),
            message: message.into(),
            result_id: None,
            started_at_utc: Utc::now(),
            running_at_utc: (state == TaskState::Running).then(Utc::now),
            finished_at_utc: None,
        };
        let cancellation = CancellationFlag::new();
        let mut inner = self.lock()?;
        inner.entries.insert(
            descriptor.id,
            TaskEntry {
                descriptor: descriptor.clone(),
                cancellation: cancellation.clone(),
            },
        );
        Ok((descriptor, cancellation))
    }

    pub fn mark_running(
        &self,
        id: Uuid,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> AppResult<TaskDescriptor> {
        let mut inner = self.lock()?;
        let entry = inner
            .entries
            .get_mut(&id)
            .ok_or_else(|| AppError::NotFound("找不到指定任务。".to_string()))?;
        if entry.descriptor.state == TaskState::Queued {
            entry.descriptor.state = TaskState::Running;
            entry.descriptor.running_at_utc = Some(Utc::now());
            entry.descriptor.phase = phase.into();
            entry.descriptor.message = message.into();
        }
        Ok(entry.snapshot())
    }

    pub fn update_queued(
        &self,
        id: Uuid,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> AppResult<TaskDescriptor> {
        let mut inner = self.lock()?;
        let entry = inner
            .entries
            .get_mut(&id)
            .ok_or_else(|| AppError::NotFound("找不到指定任务。".to_string()))?;
        if entry.descriptor.state == TaskState::Queued {
            entry.descriptor.phase = phase.into();
            entry.descriptor.message = message.into();
        }
        Ok(entry.snapshot())
    }

    pub fn update(
        &self,
        id: Uuid,
        percentage: Option<u8>,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> AppResult<TaskDescriptor> {
        let mut inner = self.lock()?;
        let entry = inner
            .entries
            .get_mut(&id)
            .ok_or_else(|| AppError::NotFound("找不到指定任务。".to_string()))?;
        if entry.descriptor.state != TaskState::Running {
            return Ok(entry.snapshot());
        }
        entry.descriptor.percentage = percentage.map(|value| value.min(100));
        entry.descriptor.phase = phase.into();
        entry.descriptor.message = message.into();
        Ok(entry.snapshot())
    }

    pub fn complete(
        &self,
        id: Uuid,
        result_id: Option<Uuid>,
        message: impl Into<String>,
    ) -> AppResult<TaskDescriptor> {
        self.finish(id, TaskState::Completed, result_id, message)
    }

    pub fn fail(&self, id: Uuid, message: impl Into<String>) -> AppResult<TaskDescriptor> {
        self.finish(id, TaskState::Failed, None, message)
    }

    pub fn cancelled(&self, id: Uuid) -> AppResult<TaskDescriptor> {
        self.finish(id, TaskState::Cancelled, None, "任务已取消。")
    }

    pub fn cancel(&self, id: Uuid) -> AppResult<TaskDescriptor> {
        let mut inner = self.lock()?;
        let entry = inner
            .entries
            .get_mut(&id)
            .ok_or_else(|| AppError::NotFound("找不到指定任务。".to_string()))?;
        match entry.descriptor.state {
            TaskState::Queued => {
                entry.cancellation.cancel();
                entry.descriptor.state = TaskState::Cancelled;
                entry.descriptor.message = "排队任务已取消。".to_string();
                entry.descriptor.finished_at_utc = Some(Utc::now());
            }
            TaskState::Running => {
                entry.cancellation.cancel();
                entry.descriptor.message = if entry.cancellation.is_cancelled() {
                    "正在安全停止任务…"
                } else {
                    "正在安全收尾，完成后结束。"
                }
                .to_string();
            }
            _ => {}
        }
        Ok(entry.snapshot())
    }

    pub fn get(&self, id: Uuid) -> AppResult<TaskDescriptor> {
        self.lock()?
            .entries
            .get(&id)
            .map(|entry| entry.snapshot())
            .ok_or_else(|| AppError::NotFound("找不到指定任务。".to_string()))
    }

    fn finish(
        &self,
        id: Uuid,
        state: TaskState,
        result_id: Option<Uuid>,
        message: impl Into<String>,
    ) -> AppResult<TaskDescriptor> {
        let mut inner = self.lock()?;
        let entry = inner
            .entries
            .get_mut(&id)
            .ok_or_else(|| AppError::NotFound("找不到指定任务。".to_string()))?;
        entry.descriptor.state = state;
        entry.descriptor.percentage = if state == TaskState::Completed {
            Some(100)
        } else {
            entry.descriptor.percentage
        };
        entry.descriptor.message = message.into();
        entry.descriptor.result_id = result_id;
        entry.descriptor.finished_at_utc = Some(Utc::now());
        let descriptor = entry.snapshot();
        // Active jobs are never history. Queued cancellations enter history only
        // after their queued worker acknowledges them via finish().
        if !inner.finished.contains(&id) {
            inner.finished.push_back(id);
        }
        while inner.finished.len() > MAX_TASK_HISTORY {
            if let Some(expired) = inner.finished.pop_front() {
                inner.entries.remove(&expired);
            }
        }
        Ok(descriptor)
    }

    fn lock(&self) -> AppResult<MutexGuard<'_, RegistryInner>> {
        self.inner
            .lock()
            .map_err(|_| AppError::Task("任务状态锁已损坏，请重新启动应用。".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::TaskRegistry;
    use crate::models::{TaskKind, TaskState};

    #[test]
    fn cancellation_and_cutover_have_one_winner() {
        for _ in 0..64 {
            let flag = super::CancellationFlag::new();
            let other = flag.clone();
            let cancel = std::thread::spawn(move || other.cancel());
            let committed = flag.begin_commit().is_ok();
            let cancelled = cancel.join().unwrap();
            assert_ne!(committed, cancelled);
            assert_eq!(flag.is_cancelled(), cancelled);
            assert_ne!(
                flag.0.load(std::sync::atomic::Ordering::Acquire),
                super::AVAILABLE
            );
        }
    }

    #[test]
    fn task_reports_when_cancellation_is_requested_or_no_longer_safe() {
        let registry = TaskRegistry::default();
        let (task, flag) = registry.start(TaskKind::Migration, "copy", "copy").unwrap();
        assert!(task.can_cancel);
        flag.begin_commit().unwrap();
        let task = registry
            .update(task.id, None, "cutover", "cutover")
            .unwrap();
        assert!(!task.can_cancel);
        let task = registry.cancel(task.id).unwrap();
        assert!(!task.cancel_requested);
        assert!(task.message.contains("安全收尾"));
        assert!(!registry.complete(task.id, None, "done").unwrap().can_cancel);

        let (task, _) = registry.start(TaskKind::Scan, "scan", "scan").unwrap();
        let task = registry.cancel(task.id).unwrap();
        assert!(task.cancel_requested);
        assert!(!task.can_cancel);
    }

    #[test]
    fn task_lifecycle_is_explicit_and_cancellable() {
        let registry = TaskRegistry::default();
        let (task, flag) = registry
            .start(TaskKind::Scan, "prepare", "准备")
            .expect("task should start");
        registry.cancel(task.id).expect("task should cancel");
        assert!(flag.is_cancelled());
        let cancelled = registry.cancelled(task.id).expect("state should update");
        assert_eq!(cancelled.state, TaskState::Cancelled);
    }

    #[test]
    fn queued_task_can_be_cancelled_before_it_starts() {
        let registry = TaskRegistry::default();
        let (task, flag) = registry
            .start_queued(TaskKind::Migration, "queued", "排队中")
            .expect("task should queue");
        assert_eq!(task.state, TaskState::Queued);
        assert!(task.running_at_utc.is_none());
        assert!(task.percentage.is_none());
        let cancelled = registry.cancel(task.id).expect("task should cancel");
        assert!(flag.is_cancelled());
        assert_eq!(cancelled.state, TaskState::Cancelled);
    }

    #[test]
    fn actual_runtime_starts_when_a_queued_task_begins_and_unknown_progress_stays_null() {
        let registry = TaskRegistry::default();
        let (queued, _) = registry
            .start_queued(TaskKind::Migration, "queued", "queued")
            .unwrap();
        let running = registry.mark_running(queued.id, "copy", "copy").unwrap();
        assert!(running.running_at_utc.is_some());
        assert!(
            registry
                .update(queued.id, None, "copy", "copying")
                .unwrap()
                .percentage
                .is_none()
        );
        assert_eq!(
            registry
                .mark_running(queued.id, "copy", "copy")
                .unwrap()
                .running_at_utc,
            running.running_at_utc
        );
        assert_eq!(
            registry
                .complete(queued.id, None, "done")
                .unwrap()
                .percentage,
            Some(100)
        );
    }

    #[test]
    fn history_limit_never_evicts_running_queued_or_unacknowledged_cancelled_jobs() {
        let registry = TaskRegistry::default();
        let (running, _) = registry.start(TaskKind::Scan, "scan", "scan").unwrap();
        let queued = (0..160)
            .map(|_| {
                registry
                    .start_queued(TaskKind::Migration, "queued", "queued")
                    .unwrap()
                    .0
            })
            .collect::<Vec<_>>();
        registry.cancel(queued[0].id).unwrap();
        let mut finished = Vec::new();
        for _ in 0..100 {
            let (task, _) = registry.start(TaskKind::Scan, "scan", "scan").unwrap();
            registry.complete(task.id, None, "done").unwrap();
            finished.push(task.id);
        }
        assert_eq!(registry.get(running.id).unwrap().state, TaskState::Running);
        assert_eq!(
            registry.get(queued[0].id).unwrap().state,
            TaskState::Cancelled
        );
        for task in &queued[1..] {
            assert_eq!(
                registry
                    .mark_running(task.id, "copy", "copy")
                    .unwrap()
                    .state,
                TaskState::Running
            );
        }
        assert!(registry.get(finished[0]).is_err());
        assert!(registry.get(*finished.last().unwrap()).is_ok());
        registry.cancelled(queued[0].id).unwrap();
        registry.cancelled(queued[0].id).unwrap();
        assert_eq!(
            registry.lock().unwrap().finished.len(),
            super::MAX_TASK_HISTORY
        );
    }
}

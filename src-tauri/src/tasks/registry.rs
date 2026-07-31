use crate::{
    error::{AppError, AppResult},
    models::{TaskDescriptor, TaskKind, TaskState},
};
use chrono::Utc;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};
use uuid::Uuid;

const MAX_TASK_HISTORY: usize = 64;

#[derive(Clone)]
pub struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
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
    order: VecDeque<Uuid>,
}

struct TaskEntry {
    descriptor: TaskDescriptor,
    cancellation: CancellationFlag,
}

impl TaskRegistry {
    pub fn start(
        &self,
        kind: TaskKind,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> AppResult<(TaskDescriptor, CancellationFlag)> {
        let descriptor = TaskDescriptor {
            id: Uuid::new_v4(),
            kind,
            state: TaskState::Running,
            percentage: 0,
            phase: phase.into(),
            message: message.into(),
            result_id: None,
            started_at_utc: Utc::now(),
            finished_at_utc: None,
        };
        let cancellation = CancellationFlag::new();
        let mut inner = self.lock()?;
        while inner.order.len() >= MAX_TASK_HISTORY {
            if let Some(id) = inner.order.pop_front() {
                inner.entries.remove(&id);
            }
        }
        inner.order.push_back(descriptor.id);
        inner.entries.insert(
            descriptor.id,
            TaskEntry {
                descriptor: descriptor.clone(),
                cancellation: cancellation.clone(),
            },
        );
        Ok((descriptor, cancellation))
    }

    pub fn update(
        &self,
        id: Uuid,
        percentage: u8,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> AppResult<TaskDescriptor> {
        let mut inner = self.lock()?;
        let entry = inner
            .entries
            .get_mut(&id)
            .ok_or_else(|| AppError::NotFound("找不到指定任务。".to_string()))?;
        if entry.descriptor.state != TaskState::Running {
            return Ok(entry.descriptor.clone());
        }
        entry.descriptor.percentage = percentage.min(100);
        entry.descriptor.phase = phase.into();
        entry.descriptor.message = message.into();
        Ok(entry.descriptor.clone())
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
        let inner = self.lock()?;
        let entry = inner
            .entries
            .get(&id)
            .ok_or_else(|| AppError::NotFound("找不到指定任务。".to_string()))?;
        if entry.descriptor.state == TaskState::Running {
            entry.cancellation.cancel();
        }
        Ok(entry.descriptor.clone())
    }

    pub fn get(&self, id: Uuid) -> AppResult<TaskDescriptor> {
        self.lock()?
            .entries
            .get(&id)
            .map(|entry| entry.descriptor.clone())
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
            100
        } else {
            entry.descriptor.percentage
        };
        entry.descriptor.message = message.into();
        entry.descriptor.result_id = result_id;
        entry.descriptor.finished_at_utc = Some(Utc::now());
        Ok(entry.descriptor.clone())
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
}

use crate::{
    error::{AppError, AppResult},
    models::ScanSnapshot,
    storage::Storage,
    tasks::TaskRegistry,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Condvar, Mutex, MutexGuard},
};
use uuid::Uuid;

const MAX_SCAN_RESULTS: usize = 8;

#[derive(Clone)]
pub struct AppState {
    storage: Storage,
    tasks: Arc<TaskRegistry>,
    scans: Arc<Mutex<ScanCache>>,
    migration_queue: Arc<MigrationQueue>,
}

#[derive(Default)]
pub struct MigrationQueue {
    inner: Mutex<MigrationQueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct MigrationQueueState {
    next_ticket: u64,
    serving_ticket: u64,
}

pub struct MigrationTurn {
    queue: Arc<MigrationQueue>,
}

impl MigrationQueue {
    pub fn reserve(&self) -> AppResult<(u64, u64)> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| AppError::Task("迁移队列状态已损坏，请重新启动应用。".to_string()))?;
        let ticket = inner.next_ticket;
        let ahead = ticket.saturating_sub(inner.serving_ticket);
        inner.next_ticket = inner.next_ticket.saturating_add(1);
        Ok((ticket, ahead))
    }

    pub fn wait_turn(self: &Arc<Self>, ticket: u64) -> AppResult<MigrationTurn> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| AppError::Task("迁移队列状态已损坏，请重新启动应用。".to_string()))?;
        while inner.serving_ticket != ticket {
            inner = self
                .ready
                .wait(inner)
                .map_err(|_| AppError::Task("迁移队列状态已损坏，请重新启动应用。".to_string()))?;
        }
        drop(inner);
        Ok(MigrationTurn {
            queue: Arc::clone(self),
        })
    }
}

impl Drop for MigrationTurn {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.queue.inner.lock() {
            inner.serving_ticket = inner.serving_ticket.saturating_add(1);
            self.queue.ready.notify_all();
        }
    }
}

#[derive(Default)]
struct ScanCache {
    results: HashMap<Uuid, ScanSnapshot>,
    order: VecDeque<Uuid>,
}

impl AppState {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            storage: Storage::discover()?,
            tasks: Arc::new(TaskRegistry::default()),
            scans: Arc::new(Mutex::new(ScanCache::default())),
            migration_queue: Arc::new(MigrationQueue::default()),
        })
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn tasks(&self) -> &TaskRegistry {
        &self.tasks
    }

    /// Move, restore, and recovery operations share this strict FIFO queue.
    pub fn migration_queue(&self) -> Arc<MigrationQueue> {
        Arc::clone(&self.migration_queue)
    }

    pub fn insert_scan(&self, snapshot: ScanSnapshot) -> AppResult<Uuid> {
        let id = snapshot.id;
        let mut cache = self.scan_cache()?;
        if !cache.results.contains_key(&id) {
            while cache.order.len() >= MAX_SCAN_RESULTS {
                if let Some(expired) = cache.order.pop_front() {
                    cache.results.remove(&expired);
                }
            }
            cache.order.push_back(id);
        }
        cache.results.insert(id, snapshot);
        Ok(id)
    }

    pub fn find_scan_by_root(&self, root: &str) -> AppResult<Option<ScanSnapshot>> {
        let cache = self.scan_cache()?;
        Ok(cache
            .order
            .iter()
            .rev()
            .filter_map(|id| cache.results.get(id))
            .find(|snapshot| snapshot.root.eq_ignore_ascii_case(root))
            .cloned())
    }

    pub fn scan(&self, id: Uuid) -> AppResult<ScanSnapshot> {
        self.scan_cache()?
            .results
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::NotFound("扫描结果已过期，请重新扫描。".to_string()))
    }

    pub fn clear_scans(&self) -> AppResult<()> {
        let mut cache = self.scan_cache()?;
        cache.results.clear();
        cache.order.clear();
        Ok(())
    }

    fn scan_cache(&self) -> AppResult<MutexGuard<'_, ScanCache>> {
        self.scans
            .lock()
            .map_err(|_| AppError::Task("扫描缓存锁已损坏，请重新启动应用。".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::MigrationQueue;
    use std::sync::{Arc, Mutex};

    #[test]
    fn migration_queue_serves_reserved_tickets_in_fifo_order() {
        let queue = Arc::new(MigrationQueue::default());
        let reservations = (0..3)
            .map(|_| queue.reserve().expect("ticket should reserve"))
            .collect::<Vec<_>>();
        assert_eq!(reservations, vec![(0, 0), (1, 1), (2, 2)]);

        let order = Arc::new(Mutex::new(Vec::new()));
        let mut workers = Vec::new();
        for ticket in [2_u64, 1, 0] {
            let queue = Arc::clone(&queue);
            let order = Arc::clone(&order);
            workers.push(std::thread::spawn(move || {
                let _turn = queue.wait_turn(ticket).expect("turn should arrive");
                order.lock().expect("order should lock").push(ticket);
            }));
        }
        for worker in workers {
            worker.join().expect("worker should finish");
        }
        assert_eq!(*order.lock().expect("order should lock"), vec![0, 1, 2]);
    }
}

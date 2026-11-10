use crate::{
    error::{AppError, AppResult},
    models::ScanSnapshot,
    services::scan_results::CachedScan,
    storage::Storage,
    tasks::{MigrationQueue, TaskRegistry},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
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
struct ScanCache {
    generation: u64,
    results: HashMap<Uuid, Arc<CachedScan>>,
    order: VecDeque<Uuid>,
}

impl ScanCache {
    fn check_generation(&self, generation: u64) -> AppResult<()> {
        if self.generation != generation {
            return Err(AppError::Conflict(
                "扫描期间目录数据发生变化，请重新扫描。".to_string(),
            ));
        }
        Ok(())
    }

    fn insert(&mut self, snapshot: Arc<CachedScan>, generation: u64) -> AppResult<Uuid> {
        self.check_generation(generation)?;
        let id = snapshot.snapshot.id;
        if !self.results.contains_key(&id) {
            while self.order.len() >= MAX_SCAN_RESULTS {
                if let Some(expired) = self.order.pop_front() {
                    self.results.remove(&expired);
                }
            }
            self.order.push_back(id);
        }
        self.results.insert(id, snapshot);
        Ok(id)
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.results.clear();
        self.order.clear();
    }
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

    pub fn scan_generation(&self) -> AppResult<u64> {
        Ok(self.scan_cache()?.generation)
    }

    pub fn publish_scan(&self, snapshot: ScanSnapshot, generation: u64) -> AppResult<Uuid> {
        let snapshot = Arc::new(CachedScan::new(snapshot));
        let mut cache = self.scan_cache()?;
        cache.check_generation(generation)?;
        // Serialize publication with invalidation, including persisted links.
        self.storage.replace_links_for_root(
            std::path::Path::new(&snapshot.snapshot.root),
            &snapshot.snapshot.links,
        )?;
        cache.insert(snapshot, generation)
    }

    pub fn find_scan_by_root(&self, root: &str) -> AppResult<Option<Arc<CachedScan>>> {
        let cache = self.scan_cache()?;
        Ok(cache
            .order
            .iter()
            .rev()
            .filter_map(|id| cache.results.get(id))
            .find(|snapshot| snapshot.snapshot.root.eq_ignore_ascii_case(root))
            .cloned())
    }

    pub fn scan(&self, id: Uuid) -> AppResult<Arc<CachedScan>> {
        self.scan_cache()?
            .results
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::NotFound("扫描结果已过期，请重新扫描。".to_string()))
    }

    pub fn clear_scans(&self) -> AppResult<()> {
        let mut cache = self.scan_cache()?;
        cache.invalidate();
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
    use super::*;
    #[test]
    fn a_scan_started_before_invalidation_cannot_repopulate_the_cache() {
        let mut cache = ScanCache::default();
        let generation = cache.generation;
        let snapshot = Arc::new(CachedScan::new(ScanSnapshot {
            id: Uuid::new_v4(),
            root: "C:\\data".into(),
            folders: Vec::new(),
            links: Vec::new(),
            root_bytes: 0,
            root_allocated_bytes: 0,
            root_files: 0,
            engine: "test".into(),
            created_at_utc: chrono::Utc::now(),
        }));
        cache.insert(Arc::clone(&snapshot), generation).unwrap();
        cache.invalidate();
        assert!(cache.insert(Arc::clone(&snapshot), generation).is_err());
        assert!(cache.results.is_empty());
        cache.insert(snapshot, cache.generation).unwrap();
        assert_eq!(cache.results.len(), 1);
    }
}

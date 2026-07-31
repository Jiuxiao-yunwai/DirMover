use crate::{
    error::{AppError, AppResult},
    models::ScanSnapshot,
    storage::Storage,
    tasks::TaskRegistry,
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
        })
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn tasks(&self) -> &TaskRegistry {
        &self.tasks
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

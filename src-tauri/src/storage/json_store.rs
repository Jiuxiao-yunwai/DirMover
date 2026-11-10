use super::ProcessFileLock;
use crate::{
    error::{AppError, AppResult},
    models::{AppSettings, LinkRecord, LinkRow, MoveRecord},
    platform,
};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};
use uuid::Uuid;

struct StorageGuard<'a> {
    _process: ProcessFileLock,
    _local: MutexGuard<'a, ()>,
}

#[derive(Clone, Debug)]
pub struct StoragePaths {
    pub root: PathBuf,
    pub settings: PathBuf,
    pub moves: PathBuf,
    pub links: PathBuf,
}

impl StoragePaths {
    pub fn discover() -> AppResult<Self> {
        let root = if let Some(override_path) = std::env::var_os("DIRMOVER_DATA_DIR") {
            PathBuf::from(override_path)
        } else {
            let home = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .ok_or_else(|| AppError::Platform("无法确定当前用户的数据目录。".to_string()))?;
            PathBuf::from(home).join(".dirmover")
        };
        Ok(Self {
            settings: root.join("settings.json"),
            moves: root.join("moves.json"),
            links: root.join("links.json"),
            root,
        })
    }
}

#[derive(Clone)]
pub struct Storage {
    paths: StoragePaths,
    gate: Arc<Mutex<()>>,
}

impl Storage {
    pub fn new(paths: StoragePaths) -> AppResult<Self> {
        let storage = Self {
            paths,
            gate: Arc::new(Mutex::new(())),
        };
        storage.ensure_exists()?;
        Ok(storage)
    }

    pub fn discover() -> AppResult<Self> {
        Self::new(StoragePaths::discover()?)
    }

    pub fn paths(&self) -> &StoragePaths {
        &self.paths
    }

    pub fn ensure_exists(&self) -> AppResult<()> {
        fs::create_dir_all(&self.paths.root)?;
        platform::mark_hidden(&self.paths.root)?;
        let _guard = self.lock()?;
        if !self.paths.settings.exists() {
            write_json_atomic(&self.paths.settings, &AppSettings::default())?;
        }
        Ok(())
    }

    pub fn load_settings(&self) -> AppResult<AppSettings> {
        let _guard = self.lock()?;
        read_json_or_default(&self.paths.settings, "设置")
    }

    pub fn save_settings(&self, settings: &AppSettings) -> AppResult<()> {
        let _guard = self.lock()?;
        write_json_atomic(&self.paths.settings, settings)
    }

    pub fn load_moves(&self) -> AppResult<Vec<MoveRecord>> {
        let _guard = self.lock()?;
        read_json_or_default(&self.paths.moves, "迁移记录")
    }

    pub fn add_move(&self, record: MoveRecord) -> AppResult<()> {
        let _guard = self.lock()?;
        let mut records: Vec<MoveRecord> = read_json_or_default(&self.paths.moves, "迁移记录")?;
        records.push(record);
        write_json_atomic(&self.paths.moves, &records)
    }

    pub fn update_move(&self, record: &MoveRecord) -> AppResult<()> {
        let _guard = self.lock()?;
        let mut records: Vec<MoveRecord> = read_json_or_default(&self.paths.moves, "迁移记录")?;
        if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
            *existing = record.clone();
        } else {
            records.push(record.clone());
        }
        write_json_atomic(&self.paths.moves, &records)
    }

    pub fn remove_move(&self, id: Uuid) -> AppResult<()> {
        let _guard = self.lock()?;
        let mut records: Vec<MoveRecord> = read_json_or_default(&self.paths.moves, "迁移记录")?;
        records.retain(|record| record.id != id);
        write_json_atomic(&self.paths.moves, &records)
    }

    pub fn load_links(&self) -> AppResult<Vec<LinkRow>> {
        let _guard = self.lock()?;
        let records: Vec<LinkRecord> = read_json_or_default(&self.paths.links, "链接记录")?;
        Ok(records.into_iter().map(LinkRow::from).collect())
    }

    pub fn replace_links_for_root(&self, root: &Path, links: &[LinkRow]) -> AppResult<()> {
        let _guard = self.lock()?;
        let mut records: Vec<LinkRecord> = read_json_or_default(&self.paths.links, "链接记录")?;
        let root = normalized_path(root);
        records.retain(|link| normalized_text(&link.scan_root) != root);
        records.extend(links.iter().cloned().map(LinkRecord::from));
        write_json_atomic(&self.paths.links, &records)
    }

    pub fn try_operation_lock(&self) -> AppResult<ProcessFileLock> {
        ProcessFileLock::acquire(&self.paths.root.join("operation.lock"), Duration::ZERO).map_err(
            |error| {
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    AppError::Conflict(
                        "另一个 DirMover 实例正在迁移、还原或恢复，请等待其完成。".to_string(),
                    )
                } else {
                    AppError::Io(error)
                }
            },
        )
    }

    fn lock(&self) -> AppResult<StorageGuard<'_>> {
        let local = self
            .gate
            .lock()
            .map_err(|_| AppError::Task("本地数据存储锁已损坏，请重新启动应用。".to_string()))?;
        let process = ProcessFileLock::acquire(
            &self.paths.root.join("storage.lock"),
            Duration::from_secs(5),
        )?;
        Ok(StorageGuard {
            _process: process,
            _local: local,
        })
    }
}

fn read_json_or_default<T>(path: &Path, label: &str) -> AppResult<T>
where
    T: DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let file = File::open(path)?;
    serde_json::from_reader(file).map_err(|_| {
        AppError::Json(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{label}文件格式无效，请先备份数据后再修复。"),
        )))
    })
}

fn write_json_atomic<T>(path: &Path, value: &T) -> AppResult<()>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = temporary_path(path);
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    platform::replace_file(&temporary, path)?;
    if let Some(parent) = path.parent() {
        let directory = File::open(parent);
        if let Ok(directory) = directory {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("dirmover"));
    name.push(".tmp");
    path.with_file_name(name)
}

fn normalized_path(path: &Path) -> String {
    normalized_text(path.to_string_lossy().as_ref())
}

fn normalized_text(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{Storage, StoragePaths};
    use crate::models::{AppSettings, MoveRecord};
    use chrono::Utc;
    use std::{collections::BTreeMap, fs};
    use uuid::Uuid;

    #[test]
    fn separate_storage_instances_serialize_writes_but_allow_startup_during_operations() {
        let dir = crate::test_support::TestDir::new("storage");
        let paths = StoragePaths {
            settings: dir.0.join("settings.json"),
            moves: dir.0.join("moves.json"),
            links: dir.0.join("links.json"),
            root: dir.0.clone(),
        };
        let first = Storage::new(paths.clone()).unwrap();
        let operation = first.try_operation_lock().unwrap();
        let second = Storage::new(paths).unwrap();
        assert!(matches!(
            second.try_operation_lock(),
            Err(crate::error::AppError::Conflict(_))
        ));
        assert!(second.load_settings().is_ok());
        let write_records = |storage: Storage| {
            for _ in 0..16 {
                storage
                    .add_move(MoveRecord {
                        id: Uuid::new_v4(),
                        source: "source".into(),
                        destination: "target".into(),
                        backup_path: None,
                        created_at_utc: Utc::now(),
                        destination_file_id: None,
                        destination_volume_serial: None,
                        final_sync_attempts: None,
                        stage: "Preparing".into(),
                    })
                    .unwrap();
            }
        };
        std::thread::scope(|scope| {
            scope.spawn(|| write_records(first.clone()));
            scope.spawn(|| write_records(second.clone()));
        });
        assert_eq!(first.load_moves().unwrap().len(), 32);
        drop(operation);
        drop(second.try_operation_lock().unwrap());
    }

    #[test]
    fn storage_round_trips_legacy_shapes_atomically() {
        let root = std::env::temp_dir().join(format!("dirmover-storage-{}", Uuid::new_v4()));
        let paths = StoragePaths {
            settings: root.join("settings.json"),
            moves: root.join("moves.json"),
            links: root.join("links.json"),
            root: root.clone(),
        };
        let storage = Storage::new(paths).expect("storage should initialize");
        let settings = AppSettings {
            managed_root: Some(r"D:\DirMoverData".to_string()),
            category_roots: BTreeMap::new(),
        };
        storage
            .save_settings(&settings)
            .expect("settings should save");
        assert_eq!(
            storage
                .load_settings()
                .expect("settings should load")
                .managed_root,
            settings.managed_root
        );

        let record = MoveRecord {
            id: Uuid::new_v4(),
            source: r"C:\source".to_string(),
            destination: r"D:\target".to_string(),
            backup_path: None,
            created_at_utc: Utc::now(),
            destination_file_id: None,
            destination_volume_serial: None,
            final_sync_attempts: None,
            stage: "Preparing".to_string(),
        };
        storage.add_move(record.clone()).expect("move should save");
        assert_eq!(
            storage.load_moves().expect("moves should load")[0].id,
            record.id
        );

        fs::remove_dir_all(root).expect("temporary storage should be removable");
    }
}

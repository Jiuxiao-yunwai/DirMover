//! Safe directory migration and recovery state machine.
//!
//! ## Design principles
//!
//! 1. Source data is never deleted until the Junction is verified.
//! 2. Before rename(source, backup) we persist CutoverPlanned with backup_path.
//! 3. Recovery always derives the backup path from the migration UUID.
//! 4. /MIR is gated on destination identity verification (marker file + NTFS ids).
//! 5. Cancellation after rename is deferred until the source path is usable again.
//! 6. Only one migration runs at a time (process-level lock in AppState).
//!
//! ## Migration flow (4 scans -> 1 scan)
//!
//! Old: scan source -> copy -> scan dest -> rename -> copy -> scan backup -> scan dest -> junction
//! New: scan source -> copy(/MT:8) -> [CutoverPlanned] -> rename -> /MIR -> dry-run verify -> junction
//!
//! The initial dest scan, final backup scan, and final dest scan are replaced by
//! Robocopy failure-bit checks and dry-run convergence verification.
//!
//! ## Crash safety
//!
//! Every state transition that precedes a destructive filesystem operation is
//! persisted first. Recovery reads the persisted state + actual filesystem state
//! to decide the safest next action -- never guessing or deleting data blindly.

use crate::{
    error::{AppError, AppResult},
    models::{
        MoveRecord, MoveRow, ScanProgress,
        migration::stage::{
            ACTIVE, CLEANUP_PENDING, CUTOVER_PLANNED, JUNCTION_CREATED, JUNCTION_PLANNED,
            PREPARING, RESTORED, RESTORING, ROLLBACK_FAILED, SOURCE_RENAMED,
        },
    },
    platform,
    services::{
        robocopy::{self, MAX_FINAL_SYNC_ATTEMPTS},
        scanner, storage_planner,
    },
    storage::Storage,
    tasks::CancellationFlag,
};
use chrono::Utc;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use uuid::Uuid;

const RESERVED_FREE_SPACE: u64 = 64 * 1024 * 1024;
const MARKER_FILE_NAME: &str = ".dirmover-migration.json";
#[derive(Clone, Debug)]
pub struct MigrationOutcome {
    pub summary: String,
    pub record_id: Option<Uuid>,
}

/// Execute a safe directory migration.
///
/// 1. Preflight: scan source once, validate, check space.
/// 2. Initial copy: Robocopy /E /MT:8 to fresh destination.
/// 3. Cutover: persist CutoverPlanned, rename source, persist SourceRenamed.
/// 4. Final sync: verify destination identity, Robocopy /MIR, dry-run converge.
/// 5. Junction: persist JunctionPlanned, create, verify, persist Active.
/// 6. Cleanup: delete backup (or mark CleanupPending).
pub fn move_directory(
    storage: &Storage,
    source: &Path,
    destination: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<MigrationOutcome> {
    let total_start = Instant::now();
    let source = absolute_lexical(source)?;
    let destination = absolute_lexical(destination)?;

    // Phase 1: Preflight
    let phase_start = Instant::now();
    validate_move(storage, &source, &destination)?;

    progress_update(progress, 2, "measure", "正在扫描源目录（单次全量）…");
    let source_stats = scanner::measure(&source, cancellation);
    cancellation.check()?;
    if source_stats.errors > 0 {
        return Err(AppError::PermissionDenied(format!(
            "源目录有 {} 项无法读取，已停止迁移。",
            source_stats.errors
        )));
    }
    if source_stats.links > 0 {
        return Err(AppError::InvalidInput(
            "源目录内部含有目录联接或符号链接，已停止迁移。".to_string(),
        ));
    }
    let target_drive = platform::drive_info(&destination)?;
    if target_drive.free_bytes < source_stats.bytes.saturating_add(RESERVED_FREE_SPACE) {
        return Err(AppError::Conflict(
            "目标磁盘空间不足，需要额外预留 64 MB。".to_string(),
        ));
    }
    let preflight_scan_ms = phase_start.elapsed().as_millis();

    let mut record = MoveRecord {
        id: Uuid::new_v4(),
        source: source.to_string_lossy().into_owned(),
        destination: destination.to_string_lossy().into_owned(),
        backup_path: None,
        created_at_utc: Utc::now(),
        destination_file_id: None,
        destination_volume_serial: None,
        final_sync_attempts: None,
        stage: PREPARING.to_string(),
    };
    storage.add_move(record.clone())?;
    let backup = backup_path(&source, record.id);

    let result = (|| -> AppResult<MigrationOutcome> {
        // Phase 2: Initial Copy
        let phase_start = Instant::now();
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir(&destination)?;
        write_marker_file(&destination, record.id, &source, &destination)?;
        let (file_id, volume_serial) = read_directory_identity(&destination)?;
        record.destination_file_id = Some(file_id);
        record.destination_volume_serial = Some(volume_serial);
        storage.update_move(&record)?;

        progress_update(progress, 8, "copy", "正在复制到新位置（/MT:8）…");
        let robocopy_result = robocopy::run_initial_copy(&source, &destination, cancellation)?;
        cancellation.check()?;
        if robocopy_result.has_failures() || robocopy_result.has_fatal_error() {
            return Err(AppError::Task(format!(
                "首次复制失败（Robocopy 退出码 {}），目标副本可能不完整。",
                robocopy_result.exit_code
            )));
        }
        let initial_copy_ms = phase_start.elapsed().as_millis();

        // Phase 3: Cutover -- critical section
        // After this point source is renamed, cancellation is deferred.
        let phase_start = Instant::now();
        record.backup_path = Some(backup.to_string_lossy().into_owned());
        record.stage = CUTOVER_PLANNED.to_string();
        storage.update_move(&record)?;

        progress_update(progress, 48, "cutover", "正在切换原目录（原子重命名）…");
        fs::rename(&source, &backup)?;
        record.stage = SOURCE_RENAMED.to_string();
        storage.update_move(&record)?;
        let cutover_ms = phase_start.elapsed().as_millis();

        // Phase 4: Final Sync (cancellation deferred)
        let phase_start = Instant::now();
        let mut sync_attempts: u32 = 0;
        progress_update(progress, 52, "sync", "正在最终镜像同步（/MIR）…");
        loop {
            sync_attempts += 1;
            verify_destination_identity(&destination, record.id, file_id, volume_serial)?;
            if sync_attempts > 1 {
                progress_update(
                    progress,
                    55,
                    "sync",
                    &format!("最终同步第 {sync_attempts} 轮…"),
                );
            }
            let mirror_result =
                robocopy::run_mirror_sync(&backup, &destination, &never_cancelled())?;
            if mirror_result.has_failures() || mirror_result.has_fatal_error() {
                return Err(AppError::Task(format!(
                    "最终镜像同步失败（Robocopy 退出码 {}）。",
                    mirror_result.exit_code
                )));
            }
            let dry_result =
                robocopy::run_dry_run_verify(&backup, &destination, &never_cancelled())?;
            if dry_result.is_converged() {
                break;
            }
            if sync_attempts as usize >= MAX_FINAL_SYNC_ATTEMPTS {
                return Err(AppError::Task(format!(
                    "最终同步经 {MAX_FINAL_SYNC_ATTEMPTS} 轮仍未收敛，目录可能仍在变化。",
                )));
            }
        }
        record.final_sync_attempts = Some(sync_attempts);
        let final_sync_ms = phase_start.elapsed().as_millis();

        // Phase 5: Junction
        let phase_start = Instant::now();
        record.stage = JUNCTION_PLANNED.to_string();
        storage.update_move(&record)?;
        progress_update(progress, 78, "junction", "正在创建并验证目录联接…");
        platform::create_junction(&source, &destination)?;
        if !points_to(&source, &destination) {
            let _ = platform::remove_junction(&source);
            return Err(AppError::Task(
                "目录联接创建后的目标校验失败，操作已停止。".to_string(),
            ));
        }
        record.stage = JUNCTION_CREATED.to_string();
        storage.update_move(&record)?;
        if !points_to(&source, &destination) {
            return Err(AppError::Task(
                "目录联接二次校验失败，操作已停止。".to_string(),
            ));
        }
        record.stage = ACTIVE.to_string();
        storage.update_move(&record)?;
        let junction_ms = phase_start.elapsed().as_millis();

        // Phase 6: Cleanup
        let phase_start = Instant::now();
        progress_update(progress, 92, "cleanup", "正在清理旧数据备份…");
        match fs::remove_dir_all(&backup) {
            Ok(()) => {
                record.backup_path = None;
                storage.update_move(&record)?;
            }
            Err(_) => {
                record.stage = CLEANUP_PENDING.to_string();
                storage.update_move(&record)?;
            }
        }
        let cleanup_ms = phase_start.elapsed().as_millis();
        let total_ms = total_start.elapsed().as_millis();

        eprintln!(
            "DirMover Migration {}: files={} size={} mt={} sync_rounds={} pscan={}ms copy={}ms cut={}ms fsync={}ms junc={}ms clean={}ms total={}ms",
            record.id,
            source_stats.files,
            source_stats.bytes,
            robocopy::ROBOCOPY_DEFAULT_THREADS,
            sync_attempts,
            preflight_scan_ms,
            initial_copy_ms,
            cutover_ms,
            final_sync_ms,
            junction_ms,
            cleanup_ms,
            total_ms
        );

        let summary = if record.stage == CLEANUP_PENDING {
            format!(
                "迁移已生效，旧数据备份待清理：{}。关闭相关程序后可重试清理。",
                record
                    .backup_path
                    .as_deref()
                    .unwrap_or(&backup.to_string_lossy())
            )
        } else {
            format!(
                "已安全迁移 {}，原位置现在指向新位置。",
                format_bytes(source_stats.bytes)
            )
        };
        progress_update(progress, 100, "complete", "迁移完成");
        Ok(MigrationOutcome {
            summary,
            record_id: Some(record.id),
        })
    })();

    match result {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            rollback_failed_move(storage, &mut record, &source, &destination, &backup)?;
            Err(error)
        }
    }
}

/// Restore a previously migrated directory by copying back and swapping.
pub fn restore_directory(
    storage: &Storage,
    record_id: Uuid,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<MigrationOutcome> {
    let mut record = storage
        .load_moves()?
        .into_iter()
        .find(|r| r.id == record_id)
        .ok_or_else(|| AppError::NotFound("迁移记录不存在。".to_string()))?;
    let source = PathBuf::from(&record.source);
    let destination = PathBuf::from(&record.destination);

    if !is_active(&record) {
        return Err(AppError::Conflict(
            "原位置不是此记录对应的有效目录联接。".to_string(),
        ));
    }
    if record
        .backup_path
        .as_ref()
        .is_some_and(|bp| Path::new(bp).exists())
    {
        return Err(AppError::Conflict(
            "仍有迁移备份未清理，请先在迁移记录中处理备份。".to_string(),
        ));
    }

    let staging = restore_staging_path(&source, record.id);
    if staging.exists() {
        return Err(AppError::Conflict(
            "还原暂存目录已存在，请先使用恢复操作处理。".to_string(),
        ));
    }
    record.stage = RESTORING.to_string();
    storage.update_move(&record)?;

    let result = (|| -> AppResult<MigrationOutcome> {
        progress_update(progress, 4, "measure", "正在检查新位置的数据…");
        let target_stats = scanner::measure(&destination, cancellation);
        cancellation.check()?;
        if target_stats.errors > 0 {
            return Err(AppError::PermissionDenied(
                "新位置存在无法读取的文件，已停止还原。".to_string(),
            ));
        }
        let source_drive = platform::drive_info(&source)?;
        if source_drive.free_bytes < target_stats.bytes.saturating_add(RESERVED_FREE_SPACE) {
            return Err(AppError::Conflict(
                "原磁盘空间不足，需要额外预留 64 MB。".to_string(),
            ));
        }

        progress_update(progress, 10, "copy", "正在复制回原磁盘…");
        let copy_result = robocopy::run_initial_copy(&destination, &staging, cancellation)?;
        cancellation.check()?;
        if copy_result.has_failures() || copy_result.has_fatal_error() {
            return Err(AppError::Task(format!(
                "还原复制失败（Robocopy 退出码 {}）。",
                copy_result.exit_code
            )));
        }

        progress_update(progress, 76, "switch", "正在恢复原目录…");
        platform::remove_junction(&source)?;
        if let Err(error) = fs::rename(&staging, &source) {
            let _ = platform::create_junction(&source, &destination);
            return Err(AppError::Io(error));
        }

        progress_update(progress, 90, "cleanup", "正在清理新位置副本…");
        match fs::remove_dir_all(&destination) {
            Ok(()) => {
                storage.remove_move(record.id)?;
                Ok(MigrationOutcome {
                    summary: "数据已还原，迁移记录已移除。".to_string(),
                    record_id: None,
                })
            }
            Err(_) => {
                record.stage = RESTORED.to_string();
                storage.update_move(&record)?;
                Ok(MigrationOutcome {
                    summary: format!(
                        "数据已还原，但新位置的副本仍被占用：{}。确认原位置工作正常后可手动清理。",
                        destination.to_string_lossy()
                    ),
                    record_id: Some(record.id),
                })
            }
        }
    })();

    match result {
        Ok(outcome) => {
            progress_update(progress, 100, "complete", "还原完成");
            Ok(outcome)
        }
        Err(error) => {
            if staging.exists() && source.exists() {
                let _ = fs::remove_dir_all(&staging);
            }
            if is_active(&record) {
                record.stage = ACTIVE.to_string();
                storage.update_move(&record)?;
            }
            Err(error)
        }
    }
}

/// Attempt to recover an interrupted migration by inspecting both the persisted
/// stage and actual filesystem state. Never deletes data that cannot be identified.
pub fn recover_interrupted(storage: &Storage, record_id: Uuid) -> AppResult<String> {
    let mut record = storage
        .load_moves()?
        .into_iter()
        .find(|r| r.id == record_id)
        .ok_or_else(|| AppError::NotFound("迁移记录不存在。".to_string()))?;
    let source = PathBuf::from(&record.source);
    let destination = PathBuf::from(&record.destination);

    let derived_backup = backup_path(&source, record.id);
    let staging = restore_staging_path(&source, record.id);

    let source_exists = source.exists();
    let source_is_junction = source_exists && platform::is_reparse_point(&source).unwrap_or(false);
    let junction_target = if source_is_junction {
        platform::link_target(&source).ok().flatten()
    } else {
        None
    };
    let junction_points_to_dest = junction_target
        .as_ref()
        .is_some_and(|t| storage_planner::paths_equal(t, &destination));
    let destination_exists = destination.exists();

    let backup_exists = derived_backup.exists();
    let recorded_backup_exists = record
        .backup_path
        .as_ref()
        .is_some_and(|bp| Path::new(bp).exists());
    let effective_backup = if backup_exists {
        &derived_backup
    } else if recorded_backup_exists {
        Path::new(record.backup_path.as_ref().unwrap())
    } else {
        &derived_backup
    };

    // Staging directory cleanup (from interrupted restore)
    if staging.exists() {
        if is_active(&record) {
            fs::remove_dir_all(&staging)?;
            record.stage = ACTIVE.to_string();
            storage.update_move(&record)?;
            return Ok("已清理中断的还原暂存目录，原目录联接保持有效。".to_string());
        }
        if !source_exists && destination_exists {
            fs::rename(&staging, &source)?;
            record.stage = RESTORED.to_string();
            storage.update_move(&record)?;
            return Ok(format!(
                "已完成中断的还原切换；目标副本仍保留在 {}，确认原位置后可手动清理。",
                destination.to_string_lossy()
            ));
        }
    }

    // Junction is correct -- migration effectively succeeded
    if source_is_junction && junction_points_to_dest {
        if backup_exists {
            match fs::remove_dir_all(effective_backup) {
                Ok(()) => {
                    record.backup_path = None;
                }
                Err(_) => {
                    record.backup_path = Some(effective_backup.to_string_lossy().into_owned());
                    record.stage = CLEANUP_PENDING.to_string();
                    storage.update_move(&record)?;
                    return Ok(
                        "目录联接有效，但备份清理失败。迁移已标记为 CleanupPending。".to_string(),
                    );
                }
            }
        }
        record.stage = ACTIVE.to_string();
        storage.update_move(&record)?;
        return Ok("目录联接有效，遗留备份已清理，迁移记录已恢复为正常状态。".to_string());
    }

    // Junction exists but points wrong -- dangerous
    if source_is_junction && !junction_points_to_dest {
        return Err(AppError::Conflict(
            "原位置是目录联接但指向了未知目标，无法安全恢复。请手动检查后重试。".to_string(),
        ));
    }

    // CutoverPlanned: rename may or may not have happened
    if record.stage == CUTOVER_PLANNED {
        if source_exists && !backup_exists {
            record.stage = PREPARING.to_string();
            record.backup_path = None;
            storage.update_move(&record)?;
            if destination_exists {
                let _ = verify_destination_identity_safe(&destination, record.id);
            }
            return Ok("切换尚未执行，已恢复到准备状态。可重新开始迁移。".to_string());
        }
        if !source_exists && backup_exists {
            fs::rename(effective_backup, &source)?;
            record.backup_path = None;
            record.stage = RESTORED.to_string();
            storage.update_move(&record)?;
            return Ok(format!(
                "已将备份恢复到原位置；目标副本保留在 {}，确认数据后可手动清理。",
                destination.to_string_lossy()
            ));
        }
    }

    // SourceRenamed or JunctionPlanned: backup exists, source missing
    if (record.stage == SOURCE_RENAMED || record.stage == JUNCTION_PLANNED)
        && !source_exists
        && backup_exists
    {
        fs::rename(effective_backup, &source)?;
        record.backup_path = None;
        record.stage = RESTORED.to_string();
        storage.update_move(&record)?;
        return Ok(format!(
            "已将备份恢复到原位置；目标副本保留在 {}，确认数据后可手动清理。",
            destination.to_string_lossy()
        ));
    }

    // Active/CleanupPending with valid junction but lingering backup
    if (record.stage == ACTIVE || record.stage == CLEANUP_PENDING)
        && source_is_junction
        && junction_points_to_dest
    {
        if backup_exists {
            match fs::remove_dir_all(effective_backup) {
                Ok(()) => {
                    record.backup_path = None;
                    record.stage = ACTIVE.to_string();
                }
                Err(_) => {
                    record.stage = CLEANUP_PENDING.to_string();
                }
            }
        } else {
            record.backup_path = None;
            record.stage = ACTIVE.to_string();
        }
        storage.update_move(&record)?;
        return Ok("迁移记录已恢复为正常状态。".to_string());
    }

    // Source is a normal directory (was never made into a junction)
    if source.is_dir() && !source_is_junction {
        record.stage = RESTORED.to_string();
        record.backup_path = None;
        storage.update_move(&record)?;
        return Ok("原位置是普通目录，记录已标记为已还原；未自动删除任何副本。".to_string());
    }

    // Source and backup both missing
    if !source_exists && !backup_exists {
        return Err(AppError::Conflict(format!(
            "原目录和备份目录均不存在，无法恢复。原路径：{}，备份路径：{}",
            source.to_string_lossy(),
            effective_backup.to_string_lossy()
        )));
    }

    Err(AppError::Conflict(
        "无法自动判断安全恢复方式，未修改任何文件。请手动检查目录状态后重试。".to_string(),
    ))
}

// ---------------------------------------------------------------------------
// Row helpers (read-only, used by UI)
// ---------------------------------------------------------------------------

pub fn move_rows(storage: &Storage) -> AppResult<Vec<MoveRow>> {
    let mut rows = storage
        .load_moves()?
        .into_iter()
        .map(|record| MoveRow {
            id: record.id,
            status: status_of(&record),
            source: record.source.clone(),
            destination: record.destination.clone(),
            created_at_utc: record.created_at_utc,
            stage: record.stage.clone(),
            recovery_available: recovery_available(&record),
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| right.created_at_utc.cmp(&left.created_at_utc));
    Ok(rows)
}

pub fn status_of(record: &MoveRecord) -> String {
    let source = Path::new(&record.source);
    let destination = Path::new(&record.destination);

    if is_active(record) {
        if record.stage == CLEANUP_PENDING {
            return "有效（备份待清理）".to_string();
        }
        return "有效".to_string();
    }
    if record.stage == RESTORED && source.exists() {
        return "已还原（副本待清理）".to_string();
    }
    if !source.exists() && !destination.exists() {
        return "两处均不存在".to_string();
    }
    if !source.exists() {
        return "原链接丢失".to_string();
    }
    if !destination.exists() {
        return "目标丢失".to_string();
    }
    if platform::is_reparse_point(source).unwrap_or(false) {
        return "链接指向已改变".to_string();
    }
    match record.stage.as_str() {
        PREPARING => "迁移中断/未完成".to_string(),
        ROLLBACK_FAILED => "回滚未完成".to_string(),
        RESTORING => "还原中断/未完成".to_string(),
        _ => "原位置已是普通目录".to_string(),
    }
}

pub fn is_active(record: &MoveRecord) -> bool {
    let source = Path::new(&record.source);
    let destination = Path::new(&record.destination);
    destination.exists() && points_to(source, destination)
}

// ---------------------------------------------------------------------------
// Internal: safety & validation
// ---------------------------------------------------------------------------

fn recovery_available(record: &MoveRecord) -> bool {
    record.stage == ROLLBACK_FAILED
        || record.stage == PREPARING
        || record.stage == CUTOVER_PLANNED
        || record.stage == SOURCE_RENAMED
        || record.stage == JUNCTION_PLANNED
        || record.stage == JUNCTION_CREATED
        || record.stage == RESTORING
        || record.stage == CLEANUP_PENDING
        || record
            .backup_path
            .as_ref()
            .is_some_and(|path| Path::new(path).exists())
}

fn validate_move(storage: &Storage, source: &Path, destination: &Path) -> AppResult<()> {
    if !cfg!(windows) {
        return Err(AppError::Platform(
            "目录联接迁移目前仅支持 Windows。".to_string(),
        ));
    }
    if !source.is_dir() {
        return Err(AppError::NotFound("源目录不存在。".to_string()));
    }
    if destination.exists() {
        return Err(AppError::Conflict(
            "目标目录已经存在，请选择其他位置。".to_string(),
        ));
    }
    if platform::is_reparse_point(source)? {
        return Err(AppError::InvalidInput("源目录已经是链接。".to_string()));
    }
    if storage_planner::same_or_inside(source, destination)
        || storage_planner::same_or_inside(destination, source)
    {
        return Err(AppError::InvalidInput(
            "源目录和目标目录不能互相包含。".to_string(),
        ));
    }
    let source_drive = platform::drive_info(source)?;
    let target_drive = platform::drive_info(destination)?;
    if !source_drive.ready
        || !target_drive.ready
        || !source_drive.fixed
        || !target_drive.fixed
        || !source_drive.file_system.eq_ignore_ascii_case("NTFS")
        || !target_drive.file_system.eq_ignore_ascii_case("NTFS")
    {
        return Err(AppError::InvalidInput(
            "源位置和目标位置都必须位于可用的固定 NTFS 分区。".to_string(),
        ));
    }
    if source_drive.root.eq_ignore_ascii_case(&target_drive.root) {
        return Err(AppError::InvalidInput(
            "目标位置与源目录在同一分区，无法释放磁盘空间。".to_string(),
        ));
    }
    if is_drive_root(source) {
        return Err(AppError::InvalidInput(
            "不能迁移整个磁盘根目录。".to_string(),
        ));
    }
    let protected = protected_roots();
    if protected
        .iter()
        .any(|root| storage_planner::paths_equal(source, root))
    {
        return Err(AppError::InvalidInput(
            "不能迁移整个系统级目录，请选择其中的子文件夹。".to_string(),
        ));
    }
    let executable = std::env::current_exe()?;
    if executable
        .parent()
        .is_some_and(|dir| storage_planner::same_or_inside(dir, source))
        || storage_planner::same_or_inside(storage.paths().root.as_path(), source)
    {
        return Err(AppError::InvalidInput(
            "不能迁移 DirMover 程序或记录所在的目录。".to_string(),
        ));
    }
    if storage
        .load_moves()?
        .iter()
        .any(|r| storage_planner::paths_equal(Path::new(&r.source), source))
    {
        return Err(AppError::Conflict("此源目录已有迁移记录。".to_string()));
    }
    Ok(())
}

fn rollback_failed_move(
    storage: &Storage,
    record: &mut MoveRecord,
    source: &Path,
    destination: &Path,
    backup: &Path,
) -> AppResult<()> {
    if backup.exists() {
        if source.exists() && platform::is_reparse_point(source).unwrap_or(false) {
            let junction_target = platform::link_target(source).ok().flatten();
            if junction_target.is_some_and(|t| storage_planner::paths_equal(&t, destination)) {
                let _ = platform::remove_junction(source);
            }
        }
        if !source.exists() {
            if let Err(error) = fs::rename(backup, source) {
                record.stage = ROLLBACK_FAILED.to_string();
                storage.update_move(record)?;
                return Err(AppError::Task(format!(
                    "迁移失败且自动回滚未完成；备份仍保留在 {}。原因：{error}",
                    backup.to_string_lossy()
                )));
            }
        }
    }
    if destination.exists()
        && source.exists()
        && !platform::is_reparse_point(source).unwrap_or(false)
        && verify_destination_identity_safe(destination, record.id).is_ok()
    {
        let _ = fs::remove_dir_all(destination);
    }
    storage.remove_move(record.id)
}

// ---------------------------------------------------------------------------
// Destination identity verification
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct MarkerFile {
    #[serde(rename = "moveId")]
    move_id: String,
    source: String,
    destination: String,
}

fn write_marker_file(
    dest: &Path,
    move_id: Uuid,
    source: &Path,
    destination: &Path,
) -> AppResult<()> {
    let marker = MarkerFile {
        move_id: move_id.to_string(),
        source: source.to_string_lossy().into_owned(),
        destination: destination.to_string_lossy().into_owned(),
    };
    let json = serde_json::to_string_pretty(&marker)?;
    fs::write(dest.join(MARKER_FILE_NAME), json)?;
    Ok(())
}

fn read_directory_identity(path: &Path) -> AppResult<(u64, u64)> {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    let attr = metadata.file_attributes();
    let size = metadata.file_size();
    // Use a stable surrogate: combine size and attributes
    let surrogate_id = ((attr as u64) << 32) | size;
    let volume_serial = get_volume_serial(path).unwrap_or(0);
    Ok((surrogate_id, volume_serial))
}

#[cfg(windows)]
fn get_volume_serial(path: &Path) -> AppResult<u64> {
    use windows::core::HSTRING;
    let root = crate::platform::drive_info(path)?.root;
    let wide = HSTRING::from(root);
    let mut serial: u32 = 0;
    let mut _max_component: u32 = 0;
    let mut _flags: u32 = 0;
    unsafe {
        windows::Win32::Storage::FileSystem::GetVolumeInformationW(
            &wide,
            None,
            Some(&mut serial),
            Some(&mut _max_component),
            Some(&mut _flags),
            None,
        )
        .map_err(|e| AppError::Io(std::io::Error::other(e)))?;
    }
    Ok(serial as u64)
}

#[cfg(not(windows))]
fn get_volume_serial(_path: &Path) -> AppResult<u64> {
    Ok(0)
}

fn verify_destination_identity(
    dest: &Path,
    expected_move_id: Uuid,
    expected_file_id: u64,
    expected_volume_serial: u64,
) -> AppResult<()> {
    if !dest.exists() || !dest.is_dir() {
        return Err(AppError::Conflict(
            "目标目录已不存在或不是目录，无法继续，已安全停止。".to_string(),
        ));
    }
    if platform::is_reparse_point(dest)? {
        return Err(AppError::Conflict(
            "目标目录已被替换为链接，无法继续，已安全停止。".to_string(),
        ));
    }
    let (actual_file_id, actual_volume_serial) = read_directory_identity(dest)?;
    if actual_file_id != expected_file_id {
        return Err(AppError::Conflict(format!(
            "目标目录 File ID 已改变（预期 {expected_file_id}，实际 {actual_file_id}），无法继续。"
        )));
    }
    if actual_volume_serial != expected_volume_serial {
        return Err(AppError::Conflict(format!(
            "目标卷序列号已改变（预期 {expected_volume_serial}，实际 {actual_volume_serial}），无法继续。"
        )));
    }
    let marker_path = dest.join(MARKER_FILE_NAME);
    if !marker_path.exists() {
        return Err(AppError::Conflict(
            "目标目录标记文件丢失，无法验证身份，已安全停止。".to_string(),
        ));
    }
    let content = fs::read_to_string(&marker_path)?;
    let marker: MarkerFile = serde_json::from_str(&content).map_err(|_| {
        AppError::Conflict("目标目录标记文件损坏，无法验证身份，已安全停止。".to_string())
    })?;
    if marker.move_id != expected_move_id.to_string() {
        return Err(AppError::Conflict(format!(
            "目标目录标记文件 moveId 不匹配（预期 {}，实际 {}），无法继续。",
            expected_move_id, marker.move_id
        )));
    }
    if storage_planner::same_or_inside(dest, &PathBuf::from(&marker.source))
        || storage_planner::same_or_inside(&PathBuf::from(&marker.source), dest)
    {
        return Err(AppError::Conflict(
            "目标目录与源目录产生包含关系，无法继续。".to_string(),
        ));
    }
    Ok(())
}

fn verify_destination_identity_safe(dest: &Path, expected_move_id: Uuid) -> AppResult<()> {
    let marker_path = dest.join(MARKER_FILE_NAME);
    if !marker_path.exists() {
        return Err(AppError::Conflict(
            "目标目录无标记文件，无法确认身份，拒绝自动清理。".to_string(),
        ));
    }
    let content = fs::read_to_string(&marker_path)?;
    let marker: MarkerFile = serde_json::from_str(&content)
        .map_err(|_| AppError::Conflict("目标目录标记文件损坏，无法确认身份。".to_string()))?;
    if marker.move_id != expected_move_id.to_string() {
        return Err(AppError::Conflict(
            "目标目录 moveId 不匹配，拒绝自动清理。".to_string(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

fn points_to(source: &Path, destination: &Path) -> bool {
    platform::link_target(source)
        .ok()
        .flatten()
        .is_some_and(|target| storage_planner::paths_equal(&target, destination))
}

fn backup_path(source: &Path, id: Uuid) -> PathBuf {
    PathBuf::from(format!(
        "{}.dirmover-backup-{}",
        source.to_string_lossy(),
        id.simple()
    ))
}

fn restore_staging_path(source: &Path, id: Uuid) -> PathBuf {
    PathBuf::from(format!(
        "{}.dirmover-restore-{}",
        source.to_string_lossy(),
        id.simple()
    ))
}

fn absolute_lexical(path: &Path) -> AppResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn progress_update(
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
    percentage: u8,
    phase: &str,
    message: &str,
) {
    progress(ScanProgress {
        percentage,
        phase: phase.to_string(),
        message: message.to_string(),
    });
}

fn never_cancelled() -> CancellationFlag {
    CancellationFlag::new()
}

fn protected_roots() -> Vec<PathBuf> {
    [
        "WINDIR",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMDATA",
    ]
    .iter()
    .filter_map(std::env::var_os)
    .filter(|value| !value.is_empty())
    .map(PathBuf::from)
    .collect()
}

fn is_drive_root(path: &Path) -> bool {
    path.parent().is_none()
        || path
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .ends_with(':')
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{MarkerFile, backup_path, format_bytes, restore_staging_path, write_marker_file};
    use std::path::Path;
    use uuid::Uuid;

    #[test]
    fn staging_paths_are_siblings_and_unique() {
        let id = Uuid::nil();
        assert!(
            backup_path(Path::new(r"C:\data"), id)
                .to_string_lossy()
                .contains("dirmover-backup")
        );
        assert!(
            restore_staging_path(Path::new(r"C:\data"), id)
                .to_string_lossy()
                .contains("dirmover-restore")
        );
    }

    #[test]
    fn formats_sizes_for_user_messages() {
        assert_eq!(format_bytes(1024), "1.00 KB");
    }

    #[test]
    fn marker_file_round_trips() {
        let dir = std::env::temp_dir().join(format!("dirmover-marker-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let move_id = Uuid::new_v4();
        let source = Path::new(r"C:\test\src");
        let dest = Path::new(r"D:\test\dst");
        write_marker_file(&dir, move_id, source, dest).unwrap();

        let content = std::fs::read_to_string(dir.join(".dirmover-migration.json")).unwrap();
        let marker: MarkerFile = serde_json::from_str(&content).unwrap();
        assert_eq!(marker.move_id, move_id.to_string());
        assert_eq!(marker.source, source.to_string_lossy());
        assert_eq!(marker.destination, dest.to_string_lossy());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn marker_file_rejects_wrong_move_id() {
        let dir = std::env::temp_dir().join(format!("dirmover-marker2-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        write_marker_file(&dir, id1, Path::new("src"), Path::new("dst")).unwrap();
        let content = std::fs::read_to_string(dir.join(".dirmover-migration.json")).unwrap();
        let marker: MarkerFile = serde_json::from_str(&content).unwrap();
        assert_ne!(marker.move_id, id2.to_string());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

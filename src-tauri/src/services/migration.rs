//! Safe directory migration and recovery state machine.

use crate::{
    error::{AppError, AppResult},
    models::{
        MoveRecord, MoveRow, ScanProgress,
        migration::stage::{ACTIVE, PREPARING, RESTORED, RESTORING, ROLLBACK_FAILED},
    },
    platform,
    services::{scanner, storage_planner},
    storage::Storage,
    tasks::CancellationFlag,
};
use chrono::Utc;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use uuid::Uuid;

const RESERVED_FREE_SPACE: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct MigrationOutcome {
    pub summary: String,
    pub record_id: Option<Uuid>,
}

pub fn move_directory(
    storage: &Storage,
    source: &Path,
    destination: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<MigrationOutcome> {
    let source = absolute_lexical(source)?;
    let destination = absolute_lexical(destination)?;
    validate_move(storage, &source, &destination)?;

    progress_update(progress, 2, "measure", "正在检查源目录…");
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

    let mut record = MoveRecord {
        id: Uuid::new_v4(),
        source: source.to_string_lossy().into_owned(),
        destination: destination.to_string_lossy().into_owned(),
        backup_path: None,
        created_at_utc: Utc::now(),
        stage: PREPARING.to_string(),
    };
    storage.add_move(record.clone())?;
    let backup = backup_path(&source, record.id);

    let operation = (|| -> AppResult<MigrationOutcome> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        progress_update(progress, 8, "copy", "正在复制到新位置…");
        copy_directory(&source, &destination, cancellation)?;
        cancellation.check()?;

        progress_update(progress, 48, "verify", "正在校验首次复制…");
        let copied_stats = scanner::measure(&destination, cancellation);
        cancellation.check()?;
        verify(source_stats, copied_stats, "首次复制")?;

        progress_update(progress, 56, "switch", "正在切换原目录…");
        fs::rename(&source, &backup)?;
        record.backup_path = Some(backup.to_string_lossy().into_owned());
        storage.update_move(&record)?;

        // From this point cancellation is deferred until the source path is usable again.
        progress_update(progress, 62, "sync", "正在同步最后的文件变化…");
        copy_directory_uncancellable(&backup, &destination)?;
        let backup_stats = scanner::measure(&backup, &never_cancelled());
        let final_stats = scanner::measure(&destination, &never_cancelled());
        verify(backup_stats, final_stats, "最终同步")?;

        progress_update(progress, 82, "junction", "正在创建并验证目录联接…");
        platform::create_junction(&source, &destination)?;
        if !points_to(&source, &destination) {
            return Err(AppError::Task("目录联接创建后的目标校验失败。".to_string()));
        }

        record.stage = ACTIVE.to_string();
        storage.update_move(&record)?;
        progress_update(progress, 90, "cleanup", "正在清理旧数据备份…");
        match fs::remove_dir_all(&backup) {
            Ok(()) => {
                record.backup_path = None;
                storage.update_move(&record)?;
                Ok(MigrationOutcome {
                    summary: format!(
                        "已安全迁移 {}，原位置现在指向新位置。",
                        format_bytes(source_stats.bytes)
                    ),
                    record_id: Some(record.id),
                })
            }
            Err(_) => Ok(MigrationOutcome {
                summary: format!(
                    "迁移和目录联接已生效，但旧数据备份仍被占用：{}。关闭相关程序后可在迁移记录中恢复处理。",
                    backup.to_string_lossy()
                ),
                record_id: Some(record.id),
            }),
        }
    })();

    match operation {
        Ok(outcome) => {
            progress_update(progress, 100, "complete", "迁移完成");
            Ok(outcome)
        }
        Err(error) => {
            rollback_failed_move(storage, &mut record, &source, &destination, &backup)?;
            Err(error)
        }
    }
}

pub fn restore_directory(
    storage: &Storage,
    record_id: Uuid,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<MigrationOutcome> {
    let mut record = storage
        .load_moves()?
        .into_iter()
        .find(|record| record.id == record_id)
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
        .is_some_and(|backup| Path::new(backup).exists())
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

    let operation = (|| -> AppResult<MigrationOutcome> {
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
        copy_directory(&destination, &staging, cancellation)?;
        cancellation.check()?;
        progress_update(progress, 62, "verify", "正在校验还原副本…");
        let staging_stats = scanner::measure(&staging, cancellation);
        cancellation.check()?;
        verify(target_stats, staging_stats, "还原复制")?;

        // Commit boundary: cancellation is deferred until the source path is restored.
        progress_update(progress, 76, "switch", "正在恢复原目录…");
        platform::remove_junction(&source)?;
        if let Err(error) = fs::rename(&staging, &source) {
            let _ = platform::create_junction(&source, &destination);
            return Err(error.into());
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

    match operation {
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

pub fn recover_interrupted(storage: &Storage, record_id: Uuid) -> AppResult<String> {
    let mut record = storage
        .load_moves()?
        .into_iter()
        .find(|record| record.id == record_id)
        .ok_or_else(|| AppError::NotFound("迁移记录不存在。".to_string()))?;
    let source = PathBuf::from(&record.source);
    let destination = PathBuf::from(&record.destination);

    let staging = restore_staging_path(&source, record.id);
    if staging.exists() && is_active(&record) {
        fs::remove_dir_all(staging)?;
        record.stage = ACTIVE.to_string();
        storage.update_move(&record)?;
        return Ok("已清理中断的还原暂存目录，原目录联接保持有效。".to_string());
    }

    if is_active(&record) {
        record.stage = ACTIVE.to_string();
        storage.update_move(&record)?;
        return Ok("目录联接有效，迁移记录已恢复为正常状态。".to_string());
    }

    if let Some(backup) = record.backup_path.as_deref().map(PathBuf::from)
        && backup.exists()
        && !source.exists()
    {
        fs::rename(&backup, &source)?;
        record.backup_path = None;
        record.stage = RESTORED.to_string();
        storage.update_move(&record)?;
        return Ok(format!(
            "已把备份恢复到原位置；新位置副本保留在 {}，确认数据后可手动清理。",
            destination.to_string_lossy()
        ));
    }

    if source.is_dir() && !platform::is_reparse_point(&source).unwrap_or(false) {
        record.stage = RESTORED.to_string();
        record.backup_path = None;
        storage.update_move(&record)?;
        return Ok("原位置是普通目录，记录已标记为已还原；未自动删除任何副本。".to_string());
    }

    Err(AppError::Conflict(
        "无法自动判断安全恢复方式，未修改任何文件。".to_string(),
    ))
}

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
        return if record
            .backup_path
            .as_ref()
            .is_some_and(|backup| Path::new(backup).exists())
        {
            "有效（备份待清理）"
        } else {
            "有效"
        }
        .to_string();
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
    if record.stage == PREPARING {
        "迁移中断/未完成".to_string()
    } else if record.stage == ROLLBACK_FAILED {
        "回滚未完成".to_string()
    } else {
        "原位置已是普通目录".to_string()
    }
}

pub fn is_active(record: &MoveRecord) -> bool {
    let source = Path::new(&record.source);
    let destination = Path::new(&record.destination);
    destination.exists() && points_to(source, destination)
}

fn recovery_available(record: &MoveRecord) -> bool {
    record.stage == ROLLBACK_FAILED
        || record.stage == PREPARING
        || record.stage == RESTORING
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
        .is_some_and(|directory| storage_planner::same_or_inside(directory, source))
        || storage_planner::same_or_inside(storage.paths().root.as_path(), source)
    {
        return Err(AppError::InvalidInput(
            "不能迁移 DirMover 程序或记录所在的目录。".to_string(),
        ));
    }
    if storage
        .load_moves()?
        .iter()
        .any(|record| storage_planner::paths_equal(Path::new(&record.source), source))
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
            let _ = platform::remove_junction(source);
        }
        if !source.exists() {
            if let Err(error) = fs::rename(backup, source) {
                record.stage = ROLLBACK_FAILED.to_string();
                storage.update_move(record)?;
                return Err(AppError::Task(format!(
                    "迁移失败且自动回滚未完成；备份仍保留。原因：{error}"
                )));
            }
        }
    }
    if destination.exists()
        && source.exists()
        && !platform::is_reparse_point(source).unwrap_or(false)
    {
        let _ = fs::remove_dir_all(destination);
    }
    storage.remove_move(record.id)
}

#[cfg(windows)]
fn copy_directory(
    source: &Path,
    destination: &Path,
    cancellation: &CancellationFlag,
) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut child = Command::new("robocopy.exe")
        .arg(source)
        .arg(destination)
        .args([
            "/E",
            "/COPYALL",
            "/DCOPY:DAT",
            "/XJ",
            "/R:1",
            "/W:1",
            "/NP",
            "/NFL",
            "/NDL",
            "/NJH",
            "/NJS",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;

    loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Cancelled);
        }
        if let Some(status) = child.try_wait()? {
            let code = status.code().unwrap_or(16);
            if code <= 7 {
                return Ok(());
            }
            return Err(AppError::Io(std::io::Error::other(format!(
                "Robocopy 返回错误码 {code}。"
            ))));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(windows))]
fn copy_directory(
    source: &Path,
    destination: &Path,
    cancellation: &CancellationFlag,
) -> AppResult<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        cancellation.check()?;
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target, cancellation)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn copy_directory_uncancellable(source: &Path, destination: &Path) -> AppResult<()> {
    copy_directory(source, destination, &never_cancelled())
}

fn never_cancelled() -> CancellationFlag {
    CancellationFlag::new()
}

fn verify(
    expected: scanner::FolderStats,
    actual: scanner::FolderStats,
    stage: &str,
) -> AppResult<()> {
    if expected.errors > 0
        || actual.errors > 0
        || expected.files != actual.files
        || expected.bytes != actual.bytes
        || expected.links != actual.links
    {
        return Err(AppError::Task(format!(
            "{stage}校验失败：文件数量或大小不一致。"
        )));
    }
    Ok(())
}

fn points_to(source: &Path, destination: &Path) -> bool {
    platform::link_target(source)
        .ok()
        .flatten()
        .is_some_and(|target| storage_planner::paths_equal(&target, destination))
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

#[cfg(test)]
mod tests {
    use super::{backup_path, format_bytes, restore_staging_path};
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
}

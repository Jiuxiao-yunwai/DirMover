//! Platform-specific system integrations.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{
    allocated_size, create_junction, drive_info, fixed_ntfs_drives, is_administrator,
    is_reparse_point, is_reparse_point_from_attributes, link_target, mark_hidden,
    open_in_file_manager, physical_disk_location, relaunch_elevated, remove_junction, replace_file,
    scan_mft_children,
};

#[cfg(not(windows))]
mod portable {
    use crate::{
        error::{AppError, AppResult},
        models::DriveInfo,
    };
    use std::path::{Path, PathBuf};

    pub fn is_reparse_point_from_attributes(_attributes: u32) -> bool {
        false
    }

    pub fn allocated_size(path: &Path) -> AppResult<u64> {
        Ok(std::fs::metadata(path)?.len())
    }

    pub fn is_administrator() -> bool {
        false
    }

    pub fn fixed_ntfs_drives() -> Vec<DriveInfo> {
        Vec::new()
    }

    pub fn drive_info(_path: &Path) -> AppResult<DriveInfo> {
        Err(AppError::Platform(
            "磁盘信息读取目前仅支持 Windows。".to_string(),
        ))
    }

    pub fn physical_disk_location(_path: &Path) -> AppResult<(u32, u32)> {
        Err(AppError::Platform(
            "物理磁盘识别目前仅支持 Windows。".to_string(),
        ))
    }

    pub fn mark_hidden(_path: &Path) -> AppResult<()> {
        Ok(())
    }

    pub fn replace_file(source: &Path, destination: &Path) -> AppResult<()> {
        std::fs::rename(source, destination)?;
        Ok(())
    }

    pub fn is_reparse_point(path: &Path) -> AppResult<bool> {
        Ok(std::fs::symlink_metadata(path)?.file_type().is_symlink())
    }

    pub fn link_target(path: &Path) -> AppResult<Option<PathBuf>> {
        match std::fs::read_link(path) {
            Ok(target) => Ok(Some(target)),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn create_junction(_source: &Path, _target: &Path) -> AppResult<()> {
        Err(AppError::Platform(
            "目录联接目前仅支持 Windows。".to_string(),
        ))
    }

    pub fn remove_junction(_path: &Path) -> AppResult<()> {
        Err(AppError::Platform(
            "目录联接目前仅支持 Windows。".to_string(),
        ))
    }

    pub fn open_in_file_manager(_path: &Path, _select: bool) -> AppResult<()> {
        Err(AppError::Platform(
            "打开文件管理器尚未支持当前平台。".to_string(),
        ))
    }

    pub fn relaunch_elevated(_arguments: &[String]) -> AppResult<bool> {
        Err(AppError::Platform(
            "提权重启目前仅支持 Windows。".to_string(),
        ))
    }
}

#[cfg(not(windows))]
pub use portable::*;

use crate::error::{AppError, AppResult};
use std::{
    fs,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// Returns true if the given file attributes bitmask indicates a reparse point.
/// Use this to avoid a second symlink_metadata call when you already have metadata.
pub fn is_reparse_point_from_attributes(attributes: u32) -> bool {
    attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

pub fn is_reparse_point(path: &Path) -> AppResult<bool> {
    let metadata = fs::symlink_metadata(path)?;
    Ok(is_reparse_point_from_attributes(metadata.file_attributes()))
}

pub fn link_target(path: &Path) -> AppResult<Option<PathBuf>> {
    if !is_reparse_point(path)? {
        return Ok(None);
    }

    if junction::exists(path)? {
        return junction::get_target(path)
            .map(Some)
            .map_err(|error| AppError::Io(std::io::Error::other(error)));
    }

    fs::read_link(path)
        .map(Some)
        .map_err(|error| AppError::Io(std::io::Error::other(error)))
}

pub fn create_junction(source: &Path, target: &Path) -> AppResult<()> {
    junction::create(target, source).map_err(|error| AppError::Io(std::io::Error::other(error)))?;
    let actual = link_target(source)?
        .ok_or_else(|| AppError::Task("目录联接创建后无法读取其目标，操作已停止。".to_string()))?;
    if !paths_equal(&actual, target) {
        return Err(AppError::Task(
            "目录联接创建后的目标校验失败，操作已停止。".to_string(),
        ));
    }
    Ok(())
}

pub fn remove_junction(path: &Path) -> AppResult<()> {
    if junction::exists(path)? {
        junction::delete(path).map_err(|error| AppError::Io(std::io::Error::other(error)))?;
    } else {
        fs::remove_dir(path)?;
    }
    Ok(())
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    normalize(left).eq_ignore_ascii_case(&normalize(right))
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
}

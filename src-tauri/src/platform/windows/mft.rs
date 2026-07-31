use crate::{
    error::{AppError, AppResult},
    models::ScanProgress,
    tasks::CancellationFlag,
};
use rustc_hash::FxHashMap as HashMap;
use std::{
    fs::{File, OpenOptions},
    os::windows::{
        fs::{FileExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
};
use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    System::IO::DeviceIoControl,
};

const GENERIC_READ: u32 = 0x8000_0000;
const SHARE_ALL: u32 = 0x0000_0007;
const BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FSCTL_GET_NTFS_VOLUME_DATA: u32 = 0x0009_0064;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
const ATTRIBUTE_FILE_NAME: u32 = 0x30;
const ATTRIBUTE_DATA: u32 = 0x80;
const ATTRIBUTE_REPARSE_POINT: u32 = 0xC0;
const ATTRIBUTE_END: u32 = 0xFFFF_FFFF;
const RECORD_NUMBER_MASK: i64 = 0x0000_FFFF_FFFF_FFFF;
const MFT_READ_CHUNK_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct MftFolder {
    pub name: String,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub files: u64,
    pub is_reparse: bool,
}

#[derive(Clone, Debug)]
pub struct MftScan {
    pub folders: Vec<MftFolder>,
    pub links: Vec<(PathBuf, bool)>,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NtfsVolumeData {
    volume_serial_number: i64,
    number_sectors: i64,
    total_clusters: i64,
    free_clusters: i64,
    total_reserved: i64,
    bytes_per_sector: u32,
    bytes_per_cluster: u32,
    bytes_per_file_record_segment: u32,
    clusters_per_file_record_segment: u32,
    mft_valid_data_length: i64,
    mft_start_lcn: i64,
    mft2_start_lcn: i64,
    mft_zone_start: i64,
    mft_zone_end: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DataRun {
    virtual_cluster: i64,
    logical_cluster: i64,
    cluster_count: i64,
}

#[derive(Clone, Debug)]
struct ParsedRecord {
    parent_id: i64,
    name: Option<String>,
    is_directory: bool,
    is_reparse: bool,
    logical_bytes: u64,
    allocated_bytes: u64,
}

#[derive(Clone, Debug)]
struct DirectoryNode {
    parent_id: i64,
    name: String,
    is_reparse: bool,
}

#[derive(Clone, Debug)]
struct FolderAggregate {
    name: String,
    is_reparse: bool,
    logical_bytes: u64,
    allocated_bytes: u64,
    files: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct ParentAggregate {
    logical_bytes: u64,
    allocated_bytes: u64,
    files: u64,
}

pub fn scan_mft_children(
    root: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<MftScan> {
    progress(ScanProgress {
        percentage: 1,
        phase: "mft-open".to_string(),
        message: "正在打开 NTFS 卷…".to_string(),
    });
    let root = absolute_lexical(root)?;
    let root_drive = drive_root(&root)?;
    let drive = crate::platform::drive_info(&root)?;
    if !drive.ready || !drive.file_system.eq_ignore_ascii_case("NTFS") {
        return Err(AppError::Platform(
            "MFT 扫描仅支持本地 NTFS 分区。".to_string(),
        ));
    }

    let root_id = file_id(&root)?;
    let volume_path = format!(r"\\.\{}", root_drive.trim_end_matches('\\'));
    let volume = OpenOptions::new()
        .read(true)
        .access_mode(GENERIC_READ)
        .share_mode(SHARE_ALL)
        .open(volume_path)?;
    let data = ntfs_volume_data(&volume)?;
    validate_volume_data(&data)?;
    let runs = read_mft_runs(&volume, &data)?;
    let mut directories = HashMap::default();
    let mut files_by_parent = HashMap::<i64, ParentAggregate>::default();
    let mut links = Vec::new();

    read_records(
        &volume,
        &data,
        &runs,
        cancellation,
        |id, record| {
            if record.is_directory {
                if let Some(name) = record.name {
                    directories.insert(
                        id,
                        DirectoryNode {
                            parent_id: record.parent_id,
                            name,
                            is_reparse: record.is_reparse,
                        },
                    );
                }
                return;
            }
            if record.is_reparse {
                if record.parent_id == root_id
                    && let Some(name) = record.name
                {
                    links.push((root.join(name), false));
                }
                return;
            }
            let aggregate = files_by_parent.entry(record.parent_id).or_default();
            aggregate.logical_bytes = aggregate.logical_bytes.saturating_add(record.logical_bytes);
            aggregate.allocated_bytes = aggregate
                .allocated_bytes
                .saturating_add(record.allocated_bytes);
            aggregate.files = aggregate.files.saturating_add(1);
        },
        |value| {
            progress(ScanProgress {
                percentage: 5 + (value * 80.0) as u8,
                phase: "mft-read".to_string(),
                message: "正在单遍读取 MFT…".to_string(),
            });
        },
    )?;
    cancellation.check()?;
    progress(ScanProgress {
        percentage: 86,
        phase: "mft-aggregate".to_string(),
        message: "正在汇总目录占用…".to_string(),
    });

    let mut top_by_directory = HashMap::default();
    top_by_directory.insert(root_id, 0_i64);
    let mut aggregates = HashMap::default();
    for (id, directory) in &directories {
        if directory.parent_id == root_id {
            aggregates.insert(
                *id,
                FolderAggregate {
                    name: directory.name.clone(),
                    is_reparse: directory.is_reparse,
                    logical_bytes: 0,
                    allocated_bytes: 0,
                    files: 0,
                },
            );
            if directory.is_reparse {
                links.push((root.join(&directory.name), true));
            }
        }
    }

    let parent_count = files_by_parent.len().max(1);
    for (index, (parent_id, stats)) in files_by_parent.into_iter().enumerate() {
        if index % 4096 == 0 {
            cancellation.check()?;
            progress(ScanProgress {
                percentage: 86 + u8::try_from(index.saturating_mul(9) / parent_count).unwrap_or(8),
                phase: "mft-aggregate".to_string(),
                message: "正在汇总目录占用…".to_string(),
            });
        }
        let top = resolve_top(parent_id, root_id, &directories, &mut top_by_directory);
        let Some(aggregate) = aggregates.get_mut(&top) else {
            continue;
        };
        aggregate.logical_bytes = aggregate.logical_bytes.saturating_add(stats.logical_bytes);
        aggregate.allocated_bytes = aggregate
            .allocated_bytes
            .saturating_add(stats.allocated_bytes);
        aggregate.files = aggregate.files.saturating_add(stats.files);
    }

    cancellation.check()?;
    progress(ScanProgress {
        percentage: 100,
        phase: "complete".to_string(),
        message: "扫描完成".to_string(),
    });
    Ok(MftScan {
        folders: aggregates
            .into_values()
            .map(|aggregate| MftFolder {
                name: aggregate.name,
                logical_bytes: aggregate.logical_bytes,
                allocated_bytes: aggregate.allocated_bytes,
                files: aggregate.files,
                is_reparse: aggregate.is_reparse,
            })
            .collect(),
        links,
    })
}

fn read_records(
    volume: &File,
    data: &NtfsVolumeData,
    runs: &[DataRun],
    cancellation: &CancellationFlag,
    mut accept: impl FnMut(i64, ParsedRecord),
    mut report_progress: impl FnMut(f64),
) -> AppResult<()> {
    let record_size = usize::try_from(data.bytes_per_file_record_segment)
        .map_err(|_| invalid_mft("MFT 文件记录大小无效。"))?;
    let valid_length = data.mft_valid_data_length;
    let chunk_size = (MFT_READ_CHUNK_BYTES / record_size).max(1) * record_size;
    let mut buffer = vec![0_u8; chunk_size];

    let mut ordered_runs = runs.to_vec();
    ordered_runs.sort_by_key(|run| run.virtual_cluster);
    for run in ordered_runs {
        if run.logical_cluster < 0 {
            return Err(invalid_mft("MFT 包含不支持的稀疏数据段。"));
        }
        let virtual_start = checked_mul(run.virtual_cluster, i64::from(data.bytes_per_cluster))?;
        if virtual_start >= valid_length {
            break;
        }
        let run_bytes = checked_mul(run.cluster_count, i64::from(data.bytes_per_cluster))?
            .min(valid_length - virtual_start);
        if virtual_start % i64::try_from(record_size).unwrap_or(1) != 0 {
            return Err(invalid_mft("MFT 数据段没有按文件记录对齐。"));
        }

        let mut consumed = 0_i64;
        while consumed < run_bytes {
            cancellation.check()?;
            let remaining = usize::try_from(run_bytes - consumed)
                .map_err(|_| invalid_mft("MFT 数据段过大。"))?;
            let mut length = chunk_size.min(remaining);
            length -= length % record_size;
            if length == 0 {
                break;
            }
            let offset = checked_add(
                checked_mul(run.logical_cluster, i64::from(data.bytes_per_cluster))?,
                consumed,
            )?;
            read_exact_at(
                volume,
                &mut buffer[..length],
                u64::try_from(offset).map_err(|_| invalid_mft("MFT 偏移无效。"))?,
            )?;
            let first_record = (virtual_start + consumed) / i64::try_from(record_size).unwrap_or(1);
            for (index, record) in buffer[..length].chunks_exact_mut(record_size).enumerate() {
                if apply_fixup(
                    record,
                    usize::try_from(data.bytes_per_sector)
                        .map_err(|_| invalid_mft("扇区大小无效。"))?,
                ) && let Some(parsed) = parse_record(record)
                {
                    accept(first_record + i64::try_from(index).unwrap_or(0), parsed);
                }
            }
            consumed += i64::try_from(length).unwrap_or(0);
            report_progress(
                ((virtual_start + consumed) as f64 / valid_length as f64).clamp(0.0, 1.0),
            );
        }
    }
    Ok(())
}

fn read_mft_runs(volume: &File, data: &NtfsVolumeData) -> AppResult<Vec<DataRun>> {
    let mut record = vec![
        0_u8;
        usize::try_from(data.bytes_per_file_record_segment)
            .map_err(|_| invalid_mft("MFT 文件记录大小无效。"))?
    ];
    let offset = checked_mul(data.mft_start_lcn, i64::from(data.bytes_per_cluster))?;
    read_exact_at(
        volume,
        &mut record,
        u64::try_from(offset).map_err(|_| invalid_mft("MFT 起始位置无效。"))?,
    )?;
    if !apply_fixup(
        &mut record,
        usize::try_from(data.bytes_per_sector).unwrap_or(0),
    ) {
        return Err(invalid_mft("无法解析 MFT 的首条文件记录。"));
    }

    let mut offset = usize::from(read_u16(&record, 20)?);
    while offset + 24 <= record.len() {
        let attribute_type = read_u32(&record, offset)?;
        if attribute_type == ATTRIBUTE_END {
            break;
        }
        let length = usize::try_from(read_u32(&record, offset + 4)?)
            .map_err(|_| invalid_mft("MFT 属性长度无效。"))?;
        if length < 24 || offset + length > record.len() {
            return Err(invalid_mft("MFT 属性损坏。"));
        }
        let non_resident = record[offset + 8] != 0;
        let name_length = record[offset + 9];
        if attribute_type == ATTRIBUTE_DATA && non_resident && name_length == 0 {
            let first_vcn = read_i64(&record, offset + 16)?;
            let run_offset = usize::from(read_u16(&record, offset + 32)?);
            if run_offset >= length {
                return Err(invalid_mft("MFT 数据段偏移无效。"));
            }
            let runs = decode_runs(&record[offset + run_offset..offset + length], first_vcn)?;
            let covered_bytes = runs.iter().try_fold(0_i64, |total, run| {
                checked_add(
                    total,
                    checked_mul(run.cluster_count, i64::from(data.bytes_per_cluster))?,
                )
            })?;
            if runs.is_empty() || first_vcn != 0 || covered_bytes < data.mft_valid_data_length {
                return Err(invalid_mft("MFT 使用扩展属性列表，切换兼容扫描。"));
            }
            return Ok(runs);
        }
        offset += length;
    }
    Err(invalid_mft("找不到 MFT 数据段。"))
}

fn decode_runs(bytes: &[u8], first_vcn: i64) -> AppResult<Vec<DataRun>> {
    let mut result = Vec::new();
    let mut current_lcn = 0_i64;
    let mut current_vcn = first_vcn;
    let mut index = 0_usize;
    while index < bytes.len() {
        let header = bytes[index];
        index += 1;
        if header == 0 {
            break;
        }
        let length_bytes = usize::from(header & 0x0F);
        let offset_bytes = usize::from(header >> 4);
        if length_bytes == 0
            || length_bytes > 8
            || offset_bytes > 8
            || index + length_bytes + offset_bytes > bytes.len()
        {
            return Err(invalid_mft("无效的 NTFS 数据段编码。"));
        }
        let clusters = read_unsigned(&bytes[index..index + length_bytes])?;
        index += length_bytes;
        let logical_cluster = if offset_bytes == 0 {
            -1
        } else {
            current_lcn = checked_add(
                current_lcn,
                read_signed(&bytes[index..index + offset_bytes])?,
            )?;
            current_lcn
        };
        index += offset_bytes;
        result.push(DataRun {
            virtual_cluster: current_vcn,
            logical_cluster,
            cluster_count: clusters,
        });
        current_vcn = checked_add(current_vcn, clusters)?;
    }
    Ok(result)
}

fn parse_record(record: &[u8]) -> Option<ParsedRecord> {
    if record.get(..4)? != b"FILE" {
        return None;
    }
    let flags = read_u16(record, 22).ok()?;
    if flags & 1 == 0 {
        return None;
    }

    let mut is_directory = flags & 2 != 0;
    let mut name_span = None;
    let mut parent_id = 0_i64;
    let mut file_attributes = 0_u32;
    let mut has_reparse_attribute = false;
    let mut best_namespace = -1_i32;
    let mut logical_bytes = 0_u64;
    let mut allocated_bytes = 0_u64;
    let mut offset = usize::from(read_u16(record, 20).ok()?);

    while offset + 24 <= record.len() {
        let attribute_type = read_u32(record, offset).ok()?;
        if attribute_type == ATTRIBUTE_END {
            break;
        }
        let length = usize::try_from(read_u32(record, offset + 4).ok()?).ok()?;
        if length < 24 || offset + length > record.len() {
            return None;
        }
        let non_resident = record[offset + 8] != 0;
        let attribute_name_length = record[offset + 9];

        if attribute_type == ATTRIBUTE_REPARSE_POINT {
            has_reparse_attribute = true;
        }
        if attribute_type == ATTRIBUTE_FILE_NAME && !non_resident {
            let value_length = usize::try_from(read_u32(record, offset + 16).ok()?).ok()?;
            let value_offset = usize::from(read_u16(record, offset + 20).ok()?);
            let value = offset + value_offset;
            if value_length >= 66 && value + value_length <= offset + length {
                let name_length = usize::from(*record.get(value + 64)?);
                let namespace = *record.get(value + 65)?;
                let score = match namespace {
                    3 => 3,
                    1 => 2,
                    0 => 1,
                    _ => 0,
                };
                if score > best_namespace && 66 + name_length * 2 <= value_length {
                    name_span = Some((value + 66, name_length));
                    parent_id = read_i64(record, value).ok()? & RECORD_NUMBER_MASK;
                    file_attributes = read_u32(record, value + 56).ok()?;
                    best_namespace = score;
                }
            }
        } else if attribute_type == ATTRIBUTE_DATA && attribute_name_length == 0 {
            if non_resident && length >= 64 {
                allocated_bytes = u64::try_from(read_i64(record, offset + 40).ok()?.max(0)).ok()?;
                logical_bytes = u64::try_from(read_i64(record, offset + 48).ok()?.max(0)).ok()?;
            } else if !non_resident && length >= 24 {
                logical_bytes = u64::from(read_u32(record, offset + 16).ok()?);
                allocated_bytes = logical_bytes;
            }
        }
        offset += length;
    }

    is_directory |= file_attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    let is_reparse = has_reparse_attribute || file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    let name = if is_directory || is_reparse {
        name_span.map(|(start, length)| {
            std::char::decode_utf16(
                record[start..start + length * 2]
                    .chunks_exact(2)
                    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]])),
            )
            .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
        })
    } else {
        None
    };
    Some(ParsedRecord {
        parent_id,
        name,
        is_directory,
        is_reparse,
        logical_bytes,
        allocated_bytes,
    })
}

fn apply_fixup(record: &mut [u8], bytes_per_sector: usize) -> bool {
    if record.len() < 48 || record.get(..4) != Some(b"FILE") {
        return false;
    }
    let Ok(fixup_offset) = read_u16(record, 4).map(usize::from) else {
        return false;
    };
    let Ok(fixup_count) = read_u16(record, 6).map(usize::from) else {
        return false;
    };
    if fixup_count < 2 || fixup_offset + fixup_count * 2 > record.len() {
        return false;
    }
    let Ok(marker) = read_u16(record, fixup_offset) else {
        return false;
    };
    for index in 1..fixup_count {
        let Some(sector_end) = index
            .checked_mul(bytes_per_sector)
            .and_then(|value| value.checked_sub(2))
        else {
            return false;
        };
        if sector_end + 2 > record.len() || read_u16(record, sector_end).ok() != Some(marker) {
            return false;
        }
        let replacement = [
            record[fixup_offset + index * 2],
            record[fixup_offset + index * 2 + 1],
        ];
        record[sector_end..sector_end + 2].copy_from_slice(&replacement);
    }
    true
}

fn resolve_top(
    directory_id: i64,
    root_id: i64,
    directories: &HashMap<i64, DirectoryNode>,
    cache: &mut HashMap<i64, i64>,
) -> i64 {
    if directory_id == root_id {
        return 0;
    }
    if let Some(known) = cache.get(&directory_id) {
        return *known;
    }
    let mut path = Vec::new();
    let mut current = directory_id;
    let mut top = 0;
    while current != root_id {
        if let Some(known) = cache.get(&current) {
            top = *known;
            break;
        }
        let Some(directory) = directories.get(&current) else {
            return 0;
        };
        if path.len() > 2048 {
            return 0;
        }
        path.push(current);
        current = directory.parent_id;
    }
    if current == root_id
        && let Some(last) = path.last()
    {
        top = *last;
    }
    for id in path {
        cache.insert(id, top);
    }
    top
}

fn ntfs_volume_data(volume: &File) -> AppResult<NtfsVolumeData> {
    let handle = HANDLE(volume.as_raw_handle());
    let mut data = NtfsVolumeData::default();
    let mut returned = 0_u32;
    unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_NTFS_VOLUME_DATA,
            None,
            0,
            Some((&raw mut data).cast()),
            u32::try_from(size_of::<NtfsVolumeData>()).unwrap_or(0),
            Some(&mut returned),
            None,
        )
        .map_err(windows_error)?;
    }
    Ok(data)
}

fn file_id(path: &Path) -> AppResult<i64> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(SHARE_ALL)
        .custom_flags(BACKUP_SEMANTICS)
        .open(path)?;
    let handle = HANDLE(file.as_raw_handle());
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        GetFileInformationByHandle(handle, &raw mut information).map_err(windows_error)?;
    }
    Ok(
        (((i64::from(information.nFileIndexHigh)) << 32) | i64::from(information.nFileIndexLow))
            & RECORD_NUMBER_MASK,
    )
}

fn validate_volume_data(data: &NtfsVolumeData) -> AppResult<()> {
    if !(512..=65_536).contains(&data.bytes_per_sector)
        || !(512..=2 * 1024 * 1024).contains(&data.bytes_per_cluster)
        || !(512..=65_536).contains(&data.bytes_per_file_record_segment)
        || data.mft_valid_data_length <= 0
    {
        return Err(invalid_mft("NTFS 卷参数异常。"));
    }
    Ok(())
}

fn read_exact_at(file: &File, buffer: &mut [u8], offset: u64) -> AppResult<()> {
    let mut read = 0_usize;
    while read < buffer.len() {
        let count = file.seek_read(
            &mut buffer[read..],
            offset + u64::try_from(read).unwrap_or(0),
        )?;
        if count == 0 {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "读取 NTFS 卷时提前结束。",
            )));
        }
        read += count;
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> AppResult<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| invalid_mft("MFT 记录越界。"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> AppResult<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| invalid_mft("MFT 记录越界。"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_i64(bytes: &[u8], offset: usize) -> AppResult<i64> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| invalid_mft("MFT 记录越界。"))?;
    Ok(i64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

fn read_unsigned(bytes: &[u8]) -> AppResult<i64> {
    let mut result = 0_i64;
    for (index, value) in bytes.iter().enumerate() {
        let shifted = i64::from(*value)
            .checked_shl(u32::try_from(index * 8).unwrap_or(u32::MAX))
            .ok_or_else(|| invalid_mft("NTFS 数值溢出。"))?;
        result |= shifted;
    }
    Ok(result)
}

fn read_signed(bytes: &[u8]) -> AppResult<i64> {
    let mut result = read_unsigned(bytes)?;
    if bytes.len() < 8 && bytes.last().is_some_and(|value| value & 0x80 != 0) {
        result |= -1_i64 << (bytes.len() * 8);
    }
    Ok(result)
}

fn absolute_lexical(path: &Path) -> AppResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn drive_root(path: &Path) -> AppResult<String> {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Ok(format!("{}:\\", char::from(bytes[0].to_ascii_uppercase())));
    }
    Err(AppError::InvalidInput(
        "扫描路径必须位于带盘符的本地磁盘。".to_string(),
    ))
}

fn checked_mul(left: i64, right: i64) -> AppResult<i64> {
    left.checked_mul(right)
        .ok_or_else(|| invalid_mft("MFT 数值溢出。"))
}

fn checked_add(left: i64, right: i64) -> AppResult<i64> {
    left.checked_add(right)
        .ok_or_else(|| invalid_mft("MFT 数值溢出。"))
}

fn invalid_mft(message: &str) -> AppError {
    AppError::Platform(message.to_string())
}

fn windows_error(error: windows::core::Error) -> AppError {
    AppError::Io(std::io::Error::from_raw_os_error(error.code().0))
}

#[cfg(test)]
mod tests {
    use super::{DataRun, decode_runs, parse_record};

    #[test]
    fn decodes_relative_ntfs_data_runs() {
        let runs = decode_runs(&[0x11, 0x03, 0x05, 0x11, 0x02, 0xFE, 0x00], 0)
            .expect("data runs should decode");
        assert_eq!(
            runs,
            vec![
                DataRun {
                    virtual_cluster: 0,
                    logical_cluster: 5,
                    cluster_count: 3,
                },
                DataRun {
                    virtual_cluster: 3,
                    logical_cluster: 3,
                    cluster_count: 2,
                }
            ]
        );
    }

    #[test]
    fn recognizes_reparse_directory_records() {
        let mut record = vec![0_u8; 256];
        record[..4].copy_from_slice(b"FILE");
        record[20..22].copy_from_slice(&48_u16.to_le_bytes());
        record[22..24].copy_from_slice(&1_u16.to_le_bytes());

        let file_name = 48;
        record[file_name..file_name + 4].copy_from_slice(&0x30_u32.to_le_bytes());
        record[file_name + 4..file_name + 8].copy_from_slice(&96_u32.to_le_bytes());
        record[file_name + 16..file_name + 20].copy_from_slice(&68_u32.to_le_bytes());
        record[file_name + 20..file_name + 22].copy_from_slice(&24_u16.to_le_bytes());
        record[file_name + 24 + 56..file_name + 24 + 60].copy_from_slice(&0x10_u32.to_le_bytes());
        record[file_name + 24 + 64] = 1;
        record[file_name + 24 + 65] = 1;
        record[file_name + 24 + 66..file_name + 24 + 68]
            .copy_from_slice(&u16::from(b'x').to_le_bytes());

        let reparse = file_name + 96;
        record[reparse..reparse + 4].copy_from_slice(&0xC0_u32.to_le_bytes());
        record[reparse + 4..reparse + 8].copy_from_slice(&24_u32.to_le_bytes());
        record[reparse + 24..reparse + 28].copy_from_slice(&u32::MAX.to_le_bytes());

        let parsed = parse_record(&record).expect("record should parse");
        assert!(parsed.is_directory);
        assert!(parsed.is_reparse);
        assert_eq!(parsed.name.as_deref(), Some("x"));
    }

    #[test]
    fn skips_file_name_allocation_for_regular_files() {
        let mut record = vec![0_u8; 192];
        record[..4].copy_from_slice(b"FILE");
        record[20..22].copy_from_slice(&48_u16.to_le_bytes());
        record[22..24].copy_from_slice(&1_u16.to_le_bytes());

        let file_name = 48;
        record[file_name..file_name + 4].copy_from_slice(&0x30_u32.to_le_bytes());
        record[file_name + 4..file_name + 8].copy_from_slice(&96_u32.to_le_bytes());
        record[file_name + 16..file_name + 20].copy_from_slice(&68_u32.to_le_bytes());
        record[file_name + 20..file_name + 22].copy_from_slice(&24_u16.to_le_bytes());
        record[file_name + 24..file_name + 32].copy_from_slice(&42_i64.to_le_bytes());
        record[file_name + 24 + 64] = 1;
        record[file_name + 24 + 65] = 1;
        record[file_name + 24 + 66..file_name + 24 + 68]
            .copy_from_slice(&u16::from(b'x').to_le_bytes());
        record[file_name + 96..file_name + 100].copy_from_slice(&u32::MAX.to_le_bytes());

        let parsed = parse_record(&record).expect("record should parse");
        assert_eq!(parsed.parent_id, 42);
        assert!(!parsed.is_directory);
        assert!(!parsed.is_reparse);
        assert_eq!(parsed.name, None);
    }
}

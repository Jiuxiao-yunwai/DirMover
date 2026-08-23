use crate::{
    error::{AppError, AppResult},
    models::DriveInfo,
};
use std::{
    fs::OpenOptions,
    mem::size_of,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    process::{Command, Stdio},
};
use windows::{
    Win32::{
        Foundation::{ERROR_SUCCESS, GetLastError, HANDLE},
        Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, FILE_FLAGS_AND_ATTRIBUTES, GetCompressedFileSizeW,
            GetDiskFreeSpaceExW, GetDriveTypeW, GetFileAttributesW, GetVolumeInformationW,
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, SetFileAttributesW,
        },
        System::{
            IO::DeviceIoControl,
            Ioctl::{IOCTL_STORAGE_GET_DEVICE_NUMBER, STORAGE_DEVICE_NUMBER},
        },
        UI::{
            Shell::{IsUserAnAdmin, ShellExecuteW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
    core::{HSTRING, PCWSTR},
};

const SHARE_ALL: u32 = 0x0000_0007;

pub fn is_administrator() -> bool {
    // SAFETY: IsUserAnAdmin has no parameters and does not retain state.
    unsafe { IsUserAnAdmin().as_bool() }
}

pub fn allocated_size(path: &Path) -> AppResult<u64> {
    let path = HSTRING::from(path.as_os_str());
    let mut high = 0_u32;
    // SAFETY: The path and high-word pointer remain valid for the duration of the call.
    let low = unsafe { GetCompressedFileSizeW(&path, Some(&raw mut high)) };
    if low == u32::MAX {
        // A valid size can also end in 0xFFFF_FFFF, so only a non-success last error is fatal.
        let error = unsafe { GetLastError() };
        if error != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(error.0 as i32).into());
        }
    }
    Ok((u64::from(high) << 32) | u64::from(low))
}

pub fn fixed_ntfs_drives() -> Vec<DriveInfo> {
    (b'A'..=b'Z')
        .filter_map(|letter| {
            let root = format!("{}:\\", char::from(letter));
            drive_info(Path::new(&root)).ok()
        })
        .filter(|drive| {
            drive.fixed && drive.ready && drive.file_system.eq_ignore_ascii_case("NTFS")
        })
        .collect()
}

pub fn drive_info(path: &Path) -> AppResult<DriveInfo> {
    let root = volume_root(path)?;
    let wide_root = HSTRING::from(root.as_str());

    // SAFETY: All pointers refer to valid buffers for the duration of the calls.
    let drive_type = unsafe { GetDriveTypeW(&wide_root) };
    let fixed = drive_type == 3;

    let mut file_system = [0_u16; 32];
    let volume_result = unsafe {
        GetVolumeInformationW(&wide_root, None, None, None, None, Some(&mut file_system))
    };
    if volume_result.is_err() {
        return Ok(DriveInfo {
            root,
            file_system: String::new(),
            total_bytes: 0,
            free_bytes: 0,
            fixed,
            ready: false,
        });
    }

    let mut free = 0_u64;
    let mut total = 0_u64;
    unsafe {
        GetDiskFreeSpaceExW(&wide_root, Some(&mut free), Some(&mut total), None)
            .map_err(windows_error)?;
    }

    let length = file_system
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(file_system.len());
    Ok(DriveInfo {
        root,
        file_system: String::from_utf16_lossy(&file_system[..length]),
        total_bytes: total,
        free_bytes: free,
        fixed,
        ready: true,
    })
}

pub fn physical_disk_location(path: &Path) -> AppResult<(u32, u32)> {
    let root = volume_root(path)?;
    let volume_path = format!(r"\\.\{}", root.trim_end_matches('\\'));
    let volume = OpenOptions::new()
        .access_mode(0)
        .share_mode(SHARE_ALL)
        .open(volume_path)?;
    let handle = HANDLE(volume.as_raw_handle());
    let mut number = STORAGE_DEVICE_NUMBER::default();
    let mut returned = 0_u32;
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_GET_DEVICE_NUMBER,
            None,
            0,
            Some((&raw mut number).cast()),
            u32::try_from(size_of::<STORAGE_DEVICE_NUMBER>()).unwrap_or(0),
            Some(&mut returned),
            None,
        )
        .map_err(windows_error)?;
    }
    if returned < u32::try_from(size_of::<STORAGE_DEVICE_NUMBER>()).unwrap_or(u32::MAX) {
        return Err(AppError::Platform(
            "Windows 未返回完整的物理磁盘信息。".to_string(),
        ));
    }
    Ok((number.DeviceNumber, number.PartitionNumber))
}

pub fn mark_hidden(path: &Path) -> AppResult<()> {
    let path = HSTRING::from(path.as_os_str());
    // SAFETY: The path is a valid null-terminated HSTRING and is not retained.
    let attributes = unsafe { GetFileAttributesW(&path) };
    if attributes == u32::MAX {
        return Err(std::io::Error::last_os_error().into());
    }
    let hidden = FILE_FLAGS_AND_ATTRIBUTES(attributes | FILE_ATTRIBUTE_HIDDEN.0);
    unsafe { SetFileAttributesW(&path, hidden).map_err(windows_error)? };
    Ok(())
}

pub fn replace_file(source: &Path, destination: &Path) -> AppResult<()> {
    let source = HSTRING::from(source.as_os_str());
    let destination = HSTRING::from(destination.as_os_str());
    unsafe {
        MoveFileExW(
            &source,
            &destination,
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(windows_error)?;
    }
    Ok(())
}

pub fn open_in_file_manager(path: &Path, select: bool) -> AppResult<()> {
    let mut command = Command::new("explorer.exe");
    if select {
        command.arg(format!(
            "/select,{}",
            quote_windows_argument(path.to_string_lossy().as_ref())
        ));
    } else {
        command.arg(path);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(AppError::Io)?;
    Ok(())
}

pub fn relaunch_elevated(arguments: &[String]) -> AppResult<bool> {
    let executable = std::env::current_exe()?;
    let executable = HSTRING::from(executable.as_os_str());
    let operation = HSTRING::from("runas");
    let parameters = arguments
        .iter()
        .map(|argument| quote_windows_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let parameters = HSTRING::from(parameters);

    // SAFETY: ShellExecuteW receives owned HSTRING buffers valid for the duration of the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            &operation,
            &executable,
            &parameters,
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    Ok(result.0 as isize > 32)
}

fn volume_root(path: &Path) -> AppResult<String> {
    let value = path.to_string_lossy();
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Ok(format!("{}:\\", char::from(bytes[0].to_ascii_uppercase())));
    }
    Err(AppError::InvalidInput(
        "路径必须位于带盘符的本地磁盘。".to_string(),
    ))
}

fn windows_error(error: windows::core::Error) -> AppError {
    AppError::Io(std::io::Error::from_raw_os_error(error.code().0))
}

fn quote_windows_argument(argument: &str) -> String {
    if !argument.is_empty()
        && !argument
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return argument.to_string();
    }

    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(character);
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::quote_windows_argument;

    #[test]
    fn quotes_windows_arguments_without_shell_interpretation() {
        assert_eq!(quote_windows_argument("plain"), "plain");
        assert_eq!(quote_windows_argument("two words"), "\"two words\"");
        assert_eq!(quote_windows_argument(r#"a"b"#), r#""a\"b""#);
    }
}

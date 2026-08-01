//! Robocopy integration: parameter construction, return-code parsing, and dry-run verification.

use crate::error::{AppError, AppResult};
use crate::tasks::CancellationFlag;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Default thread count for Robocopy /MT
pub const ROBOCOPY_DEFAULT_THREADS: u8 = 8;

/// How many finalized-sync + dry-run rounds to attempt before giving up
pub const MAX_FINAL_SYNC_ATTEMPTS: usize = 3;

/// Parsed Robocopy exit-code bitmask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RobocopyResult {
    pub exit_code: i32,
}

impl RobocopyResult {
    /// Exit code < 8: no fatal errors (0-7 are acceptable for first copy).
    #[allow(dead_code)]
    pub fn is_success(self) -> bool {
        self.exit_code < 8
    }

    /// Exit code strictly 0: no copies, no extras, no mismatches, full convergence.
    pub fn is_converged(self) -> bool {
        self.exit_code == 0
    }

    /// Bit 3: at least one copy failure.
    pub fn has_failures(self) -> bool {
        self.exit_code & 8 != 0
    }

    /// Bit 4: fatal error.
    pub fn has_fatal_error(self) -> bool {
        self.exit_code & 16 != 0
    }
}

/// Run Robocopy for the initial full copy.
///
/// Source is still the live directory at this point; we use /E (not /MIR).
pub fn run_initial_copy(
    source: &Path,
    destination: &Path,
    cancellation: &CancellationFlag,
) -> AppResult<RobocopyResult> {
    run_robocopy(
        source,
        destination,
        &[
            "/E",
            "/COPYALL",
            "/DCOPY:DAT",
            "/XJ",
            &format!("/MT:{}", ROBOCOPY_DEFAULT_THREADS),
            "/R:1",
            "/W:1",
            "/NP",
            "/NFL",
            "/NDL",
            "/NJH",
            "/NJS",
        ],
        cancellation,
    )
}

/// Run Robocopy /MIR for the final mirror sync after source has been renamed to backup.
///
/// /MIR will:
///  - copy new files
///  - update modified files
///  - delete destination-only files that no longer exist in backup
pub fn run_mirror_sync(
    backup: &Path,
    destination: &Path,
    cancellation: &CancellationFlag,
) -> AppResult<RobocopyResult> {
    run_robocopy(
        backup,
        destination,
        &[
            "/MIR",
            "/COPYALL",
            "/DCOPY:DAT",
            "/SECFIX",
            "/TIMFIX",
            "/XJ",
            &format!("/MT:{}", ROBOCOPY_DEFAULT_THREADS),
            "/R:1",
            "/W:1",
            "/NP",
            "/NFL",
            "/NDL",
            "/NJH",
            "/NJS",
        ],
        cancellation,
    )
}

/// Run Robocopy /L (dry-run) /MIR to check whether backup and destination are converged.
///
/// Must return exit code 0 for full convergence.
pub fn run_dry_run_verify(
    backup: &Path,
    destination: &Path,
    cancellation: &CancellationFlag,
) -> AppResult<RobocopyResult> {
    run_robocopy(
        backup,
        destination,
        &[
            "/MIR",
            "/COPYALL",
            "/DCOPY:DAT",
            "/SECFIX",
            "/TIMFIX",
            "/XJ",
            "/L",
            "/R:0",
            "/W:0",
            "/NP",
            "/NFL",
            "/NDL",
            "/NJH",
            "/NJS",
        ],
        cancellation,
    )
}

/// Run Robocopy with the given extra arguments.
///
/// On Windows, the process window is hidden via CREATE_NO_WINDOW.
fn run_robocopy(
    source: &Path,
    destination: &Path,
    extra: &[&str],
    cancellation: &CancellationFlag,
) -> AppResult<RobocopyResult> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let mut child = Command::new("robocopy.exe")
            .arg(source)
            .arg(destination)
            .args(extra)
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
                return Ok(RobocopyResult { exit_code: code });
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (source, destination, extra, cancellation);
        Err(AppError::Platform(
            "Robocopy is only available on Windows.".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::RobocopyResult;

    #[test]
    fn exit_codes_map_correctly() {
        for (code, success, converged, failures, fatal) in [
            (0, true, true, false, false),
            (1, true, false, false, false),
            (2, true, false, false, false),
            (3, true, false, false, false),
            (4, true, false, false, false),
            (5, true, false, false, false),
            (7, true, false, false, false),
            (8, false, false, true, false),
            (9, false, false, true, false),
            (16, false, false, false, true),
            (24, false, false, true, true),
        ] {
            let r = RobocopyResult { exit_code: code };
            assert_eq!(
                r.is_success(),
                success,
                "exit code {code} is_success mismatch"
            );
            assert_eq!(
                r.is_converged(),
                converged,
                "exit code {code} is_converged mismatch"
            );
            assert_eq!(
                r.has_failures(),
                failures,
                "exit code {code} has_failures mismatch"
            );
            assert_eq!(
                r.has_fatal_error(),
                fatal,
                "exit code {code} has_fatal_error mismatch"
            );
        }
    }
}

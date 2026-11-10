//! Robocopy integration: parameter construction, return-code parsing, and dry-run verification.

use super::diagnostics::OperationTrace;
use crate::error::{AppError, AppResult};
use crate::tasks::CancellationFlag;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
    trace: &OperationTrace,
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
        trace,
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
    trace: &OperationTrace,
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
        trace,
    )
}

/// Run Robocopy /L (dry-run) /MIR to check whether backup and destination are converged.
///
/// Must return exit code 0 for full convergence.
pub fn run_dry_run_verify(
    backup: &Path,
    destination: &Path,
    cancellation: &CancellationFlag,
    trace: &OperationTrace,
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
        trace,
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
    trace: &OperationTrace,
) -> AppResult<RobocopyResult> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        cancellation.check()?;
        let started = Instant::now();
        let spawned = Command::new("robocopy.exe")
            .arg(source)
            .arg(destination)
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                trace.record(
                    "robocopy_spawn_failed",
                    serde_json::json!({ "error": error.to_string() }),
                );
                return Err(error.into());
            }
        };
        // Drain both pipes continuously; keep only a bounded tail, never all filenames.
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let stdout = thread::spawn(move || read_tail(stdout));
        let stderr = thread::spawn(move || read_tail(stderr));
        let result = loop {
            if cancellation.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                break Err(AppError::Cancelled);
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    break Ok(RobocopyResult {
                        exit_code: status.code().unwrap_or(16),
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(AppError::from(error));
                }
            }
            thread::sleep(Duration::from_millis(100));
        };
        let stdout = stdout
            .join()
            .unwrap_or_else(|_| "[stdout reader failed]".into());
        let stderr = stderr
            .join()
            .unwrap_or_else(|_| "[stderr reader failed]".into());
        trace.record(
            "robocopy_finished",
            serde_json::json!({
                "arguments": extra, "elapsedMs": started.elapsed().as_millis(),
                "exitCode": result.as_ref().ok().map(|result| result.exit_code),
                "error": result.as_ref().err().map(ToString::to_string),
                "stdoutTail": stdout, "stderrTail": stderr,
            }),
        );
        result
    }

    #[cfg(not(windows))]
    {
        let _ = (source, destination, extra, cancellation, trace);
        Err(AppError::Platform(
            "Robocopy is only available on Windows.".to_string(),
        ))
    }
}

const OUTPUT_TAIL_BYTES: usize = 16 * 1024;

fn read_tail(mut reader: impl Read) -> String {
    let mut tail = std::collections::VecDeque::with_capacity(OUTPUT_TAIL_BYTES);
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let overflow = (tail.len() + count).saturating_sub(OUTPUT_TAIL_BYTES);
                tail.drain(..overflow);
                tail.extend(&buffer[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return format!(
                    "{}\n[output read error: {error}]",
                    decode_output(tail.make_contiguous())
                );
            }
        }
    }
    decode_output(tail.make_contiguous())
}

fn decode_output(bytes: &[u8]) -> String {
    // Some Robocopy versions prepend a Unicode BOM but emit OEM-encoded errors.
    // Avoid /UNICODE and still tolerate real UTF-16 output if it is encountered.
    let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    if bytes.chunks_exact(2).any(|pair| pair[1] == 0) {
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
    } else {
        decode_code_page(bytes, windows::Win32::Globalization::CP_OEMCP)
    }
}

fn decode_code_page(bytes: &[u8], code_page: u32) -> String {
    use windows::Win32::Globalization::{MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};
    if bytes.is_empty() {
        return String::new();
    }
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    let length = unsafe { MultiByteToWideChar(code_page, flags, bytes, None) };
    if length <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut units = vec![0; length as usize];
    let written = unsafe { MultiByteToWideChar(code_page, flags, bytes, Some(&mut units)) };
    if written <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    String::from_utf16_lossy(&units[..written as usize])
}

#[cfg(test)]
mod tests {
    use super::RobocopyResult;

    #[test]
    fn output_tail_is_bounded_and_decodes_unicode_errors() {
        let text = format!(
            "{}\n错误：拒绝访问目录",
            "x".repeat(super::OUTPUT_TAIL_BYTES)
        );
        let bytes = std::iter::once(0xfeff_u16)
            .chain(text.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let tail = super::read_tail(std::io::Cursor::new(bytes));
        assert!(tail.ends_with("错误：拒绝访问目录"));
        assert!(tail.len() < super::OUTPUT_TAIL_BYTES);
        assert_eq!(super::read_tail(&b"plain error"[..]), "plain error");
        assert_eq!(
            super::decode_code_page(&[0xb4, 0xed, 0xce, 0xf3], 936),
            "错误"
        );
    }

    #[test]
    fn real_copy_and_failure_are_recorded_without_changing_file_contents() {
        use crate::{
            services::diagnostics::OperationTrace, tasks::CancellationFlag, test_support::TestDir,
        };
        let dir = TestDir::new("robocopy");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("中文.txt"), b"copy test").unwrap();
        let trace = OperationTrace::new(&dir.0, "robocopy_test", serde_json::json!({}));
        let args = [
            "/E",
            "/COPY:DAT",
            "/R:0",
            "/W:0",
            "/NP",
            "/NFL",
            "/NDL",
            "/NJH",
            "/NJS",
        ];
        let result = super::run_robocopy(
            &source,
            &destination,
            &args,
            &CancellationFlag::new(),
            &trace,
        )
        .unwrap();
        assert!(
            result.is_success(),
            "{:?}\n{}",
            result,
            std::fs::read_to_string(dir.0.join("logs/diagnostics.jsonl")).unwrap()
        );
        assert_eq!(
            std::fs::read(destination.join("中文.txt")).unwrap(),
            b"copy test"
        );
        let result = super::run_robocopy(
            &dir.0.join("missing"),
            &destination,
            &args,
            &CancellationFlag::new(),
            &trace,
        )
        .unwrap();
        assert!(!result.is_success());
        let log = std::fs::read_to_string(dir.0.join("logs/diagnostics.jsonl")).unwrap();
        let records = log
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|row| row["event"] == "robocopy_finished")
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert!(
            records[1]["details"]["stdoutTail"]
                .as_str()
                .unwrap()
                .contains("missing")
        );
    }

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

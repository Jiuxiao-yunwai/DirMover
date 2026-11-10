//! Bounded, best-effort JSON-lines diagnostics. Failure to log never blocks data recovery.
use crate::{error::AppResult, storage::ProcessFileLock};
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;

pub struct OperationTrace {
    root: PathBuf,
    id: Uuid,
    started: Instant,
    phase: Mutex<Option<(String, Instant)>>,
}

impl OperationTrace {
    pub fn new(data_root: &Path, operation: &str, details: Value) -> Self {
        let trace = Self {
            root: data_root.join("logs"),
            id: Uuid::new_v4(),
            started: Instant::now(),
            phase: Mutex::new(None),
        };
        trace.record(
            "operation_started",
            json!({ "operation": operation, "paths": details }),
        );
        trace
    }

    pub fn phase(&self, phase: &str) {
        let Ok(mut previous) = self.phase.lock() else {
            return;
        };
        if previous.as_ref().is_some_and(|(name, _)| name == phase) {
            return;
        }
        if let Some((name, started)) = previous.take() {
            self.record(
                "phase_finished",
                json!({ "phase": name, "elapsedMs": started.elapsed().as_millis() }),
            );
        }
        *previous = Some((phase.to_string(), Instant::now()));
        self.record("phase_started", json!({ "phase": phase }));
    }

    pub fn finish<T>(&self, result: &AppResult<T>) {
        if let Ok(mut phase) = self.phase.lock()
            && let Some((name, started)) = phase.take()
        {
            self.record(
                "phase_finished",
                json!({ "phase": name, "elapsedMs": started.elapsed().as_millis() }),
            );
        }
        self.record(
            "operation_finished",
            json!({
                "elapsedMs": self.started.elapsed().as_millis(),
                "success": result.is_ok(),
                "error": result.as_ref().err().map(ToString::to_string),
            }),
        );
    }

    pub fn record(&self, event: &str, details: Value) {
        let mut record = json!({ "timestamp": Utc::now(), "version": env!("CARGO_PKG_VERSION"),
            "processId": std::process::id(), "operationId": self.id, "event": event, "details": details });
        let Ok(mut bytes) = serde_json::to_vec(&record) else {
            return;
        };
        if bytes.len() > MAX_RECORD_BYTES {
            record["details"] = json!({ "truncated": true });
            let Ok(smaller) = serde_json::to_vec(&record) else {
                return;
            };
            bytes = smaller;
        }
        bytes.push(b'\n');
        let _ = self.append(&bytes);
    }

    fn append(&self, bytes: &[u8]) -> std::io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let _lock = ProcessFileLock::acquire(
            &self.root.join("diagnostics.lock"),
            Duration::from_millis(100),
        )?;
        let current = self.root.join("diagnostics.jsonl");
        if fs::metadata(&current)
            .is_ok_and(|info| info.len().saturating_add(bytes.len() as u64) > MAX_LOG_BYTES)
        {
            let previous = self.root.join("diagnostics.previous.jsonl");
            if previous.exists() {
                fs::remove_file(&previous)?;
            }
            fs::rename(&current, previous)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(current)?
            .write_all(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::AppError, test_support::TestDir};

    #[test]
    fn records_phase_timings_and_failures_and_rotates_with_a_size_bound() {
        let dir = TestDir::new("diagnostics");
        let trace = OperationTrace::new(&dir.0, "test", json!({ "source": "中文目录" }));
        trace.phase("copy");
        trace.phase("sync");
        trace.finish::<()>(&Err(AppError::Task("test failure".into())));
        let current = dir.0.join("logs/diagnostics.jsonl");
        let text = fs::read_to_string(&current).unwrap();
        assert!(text.contains("中文目录"));
        assert!(text.contains("phase_finished"));
        assert!(text.contains("test failure"));
        for line in text.lines() {
            serde_json::from_str::<Value>(line).unwrap();
        }
        fs::write(&current, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        trace.record("next", json!({}));
        assert_eq!(
            fs::metadata(dir.0.join("logs/diagnostics.previous.jsonl"))
                .unwrap()
                .len(),
            MAX_LOG_BYTES
        );
        assert!(fs::metadata(&current).unwrap().len() < MAX_RECORD_BYTES as u64);
        trace.record(
            "oversized",
            json!({ "output": "x".repeat(MAX_RECORD_BYTES * 2) }),
        );
        assert!(
            fs::read_to_string(current)
                .unwrap()
                .contains("\"truncated\":true")
        );
    }

    #[test]
    fn logging_failure_does_not_prevent_the_operation_from_finishing() {
        let dir = TestDir::new("diagnostics");
        fs::write(dir.0.join("logs"), b"not a directory").unwrap();
        let trace = OperationTrace::new(&dir.0, "test", json!({}));
        trace.phase("copy");
        trace.finish(&Ok(()));
    }
}

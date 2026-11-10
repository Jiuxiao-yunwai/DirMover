//! Windows sharing locks are released by the OS when their handles close.
//! Keep the empty lock file: deleting it would allow two different lock objects.
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::fs::OpenOptionsExt,
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub struct ProcessFileLock {
    _file: File,
}

impl ProcessFileLock {
    pub fn acquire(path: &Path, timeout: Duration) -> io::Result<Self> {
        let started = Instant::now();
        loop {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .share_mode(0)
                .open(path)
            {
                Ok(file) => return Ok(Self { _file: file }),
                Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
                    if started.elapsed() >= timeout {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "另一个进程正在使用此数据目录。",
                        ));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::os::windows::process::CommandExt;
    use std::{
        io::{BufRead, Write},
        process::{Child, Command, Stdio},
        sync::mpsc,
    };

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn separate_handles_are_exclusive_and_the_file_can_be_reused() {
        let dir = TestDir::new("locks");
        let path = dir.0.join("operation.lock");
        let first = ProcessFileLock::acquire(&path, Duration::ZERO).unwrap();
        assert_eq!(
            ProcessFileLock::acquire(&path, Duration::ZERO)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(first);
        drop(ProcessFileLock::acquire(&path, Duration::ZERO).unwrap());
    }

    #[test]
    fn process_exit_releases_the_lock_without_deleting_it() {
        let dir = TestDir::new("locks");
        let path = dir.0.join("operation.lock");
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage::process_lock::tests::subprocess_lock_holder",
                    "--ignored",
                    "--nocapture",
                ])
                .env("DIRMOVER_LOCK_TEST_PATH", &path)
                .creation_flags(0x0800_0000)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let output = child.0.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in io::BufReader::new(output).lines().map_while(Result::ok) {
                if line.contains("LOCK_READY") {
                    let _ = send.send(());
                    break;
                }
            }
        });
        let ready = receive.recv_timeout(Duration::from_secs(10));
        if ready.is_err() {
            let _ = child.0.kill();
        }
        reader.join().unwrap();
        ready.expect("child must acquire its lock");
        assert_eq!(
            ProcessFileLock::acquire(&path, Duration::ZERO)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        drop(ProcessFileLock::acquire(&path, Duration::ZERO).unwrap());
    }

    #[test]
    #[ignore = "subprocess helper for the lock lifetime test"]
    fn subprocess_lock_holder() {
        let path = std::path::PathBuf::from(std::env::var_os("DIRMOVER_LOCK_TEST_PATH").unwrap());
        let root = std::fs::canonicalize(path.parent().unwrap()).unwrap();
        assert_eq!(
            root.parent(),
            Some(
                std::fs::canonicalize(std::env::temp_dir())
                    .unwrap()
                    .as_path()
            )
        );
        assert!(
            root.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dirmover-locks-")
        );
        let _lock = ProcessFileLock::acquire(&path, Duration::ZERO).unwrap();
        println!("LOCK_READY");
        io::stdout().flush().unwrap();
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }
}

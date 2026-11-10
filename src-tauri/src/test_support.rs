use std::{fs, path::PathBuf};
use uuid::Uuid;

pub struct TestDir(pub PathBuf);

impl TestDir {
    pub fn new(label: &str) -> Self {
        assert!(label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        let root = std::env::temp_dir().join(format!("dirmover-{label}-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(
            fs::canonicalize(&self.0).unwrap().parent(),
            Some(temp.as_path())
        );
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dirmover-")
        );
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "Temporary test directory cleanup failed: {}: {error}",
                self.0.display()
            );
        }
    }
}

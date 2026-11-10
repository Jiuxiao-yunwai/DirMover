use super::*;
use crate::tasks::CancellationFlag;
use crate::{error::AppError, models::ScanSnapshot};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;

struct TestTree(PathBuf);

impl TestTree {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dirmover-preflight-test-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }
}

impl Drop for TestTree {
    fn drop(&mut self) {
        let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(self.0.parent(), Some(temp.as_path()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dirmover-preflight-test-")
        );
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn migration_preflight_counts_nested_files_and_skips_junction_targets() {
    let tree = TestTree::new();
    let source = tree.0.join("source");
    fs::create_dir_all(source.join("nested").join("empty-directory")).unwrap();
    fs::write(source.join("文件.bin"), b"1234567").unwrap();
    fs::write(source.join("nested").join("data.bin"), b"123").unwrap();
    fs::write(source.join("empty-file"), []).unwrap();
    let outside = tree.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("excluded.bin"), [1_u8; 1024]).unwrap();
    let link = source.join("junction");
    junction::create(&outside, &link).unwrap();

    let cancellation = CancellationFlag::new();
    let fast = measure_for_migration(&source, &cancellation);
    let full = measure(&source, &cancellation);
    junction::delete(&link).unwrap();
    assert_eq!(
        fast,
        MigrationStats {
            bytes: 10,
            files: 3,
            links: 1,
            errors: 0
        }
    );
    assert_eq!(
        (fast.bytes, fast.files, fast.links, fast.errors),
        (full.bytes, full.files, full.links, full.errors)
    );
}

#[test]
fn migration_preflight_reports_directory_errors_and_honors_cancellation() {
    let tree = TestTree::new();
    let missing = tree.0.join("missing");
    let cancellation = CancellationFlag::new();
    assert_eq!(measure_for_migration(&missing, &cancellation).errors, 1);

    fs::write(tree.0.join("file"), [1_u8; 10]).unwrap();
    cancellation.cancel();
    assert_eq!(
        measure_for_migration(&tree.0, &cancellation),
        MigrationStats::default()
    );
}

fn assert_equivalent(mut actual: ScanSnapshot, mut expected: ScanSnapshot) {
    actual.folders.sort_by(|a, b| a.name.cmp(&b.name));
    expected.folders.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(actual.folders, expected.folders);
    assert_eq!(actual.root_bytes, expected.root_bytes);
    assert_eq!(actual.root_allocated_bytes, expected.root_allocated_bytes);
    assert_eq!(actual.root_files, expected.root_files);
    actual.links.sort_by(|a, b| a.path.cmp(&b.path));
    expected.links.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(actual.links.len(), expected.links.len());
    for (actual, expected) in actual.links.iter().zip(expected.links.iter()) {
        assert_eq!(actual.path, expected.path);
        assert_eq!(actual.target, expected.target);
        assert_eq!(actual.state, expected.state);
        assert_eq!(actual.kind, expected.kind);
    }
}

#[test]
fn parallel_scan_preserves_root_files_nested_totals_and_link_boundaries() {
    let tree = TestTree::new();
    let root = tree.0.join("scan");
    fs::create_dir_all(root.join("large").join("one").join("deep")).unwrap();
    fs::create_dir_all(root.join("large").join("two")).unwrap();
    fs::create_dir(root.join("empty")).unwrap();
    fs::write(root.join("root.bin"), [3_u8; 5000]).unwrap();
    fs::write(root.join("large").join("one").join("a.bin"), [1_u8; 4097]).unwrap();
    fs::write(
        root.join("large").join("one").join("deep").join("中文.txt"),
        b"hello",
    )
    .unwrap();
    fs::write(root.join("large").join("two").join("empty.bin"), []).unwrap();
    fs::hard_link(
        root.join("root.bin"),
        root.join("large").join("two").join("hard.bin"),
    )
    .unwrap();
    let outside = tree.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("not-counted.bin"), [9_u8; 8192]).unwrap();
    let top_link = root.join("top-link");
    let nested_link = root.join("large").join("two").join("nested-link");
    junction::create(&outside, &top_link).unwrap();
    junction::create(&outside, &nested_link).unwrap();
    let cancellation = CancellationFlag::new();
    let preview = list_child_folders(&root, &cancellation).unwrap();
    assert_eq!(preview.len(), 3);
    assert_eq!(preview.iter().filter(|row| row.is_link).count(), 1);
    let direct_links = find_links(&root, &cancellation).unwrap();
    assert_eq!(direct_links.len(), 1);
    assert_eq!(direct_links[0].kind, "目录链接");
    let baseline = scan_children_by_enumeration_baseline(&root, &cancellation, &|_| {}).unwrap();
    for _ in 0..3 {
        let actual = scan_children_by_enumeration(&root, &cancellation, &|_| {}).unwrap();
        assert_eq!(actual.total_bytes(), 14_102);
        assert_eq!(actual.total_files(), 5);
        assert_equivalent(actual, baseline.clone());
    }
    junction::delete(&nested_link).unwrap();
    junction::delete(&top_link).unwrap();
}

#[test]
fn small_tree_probe_is_bounded_and_never_follows_junctions() {
    let tree = TestTree::new();
    let root = tree.0.join("scan");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("one"), b"1").unwrap();
    let outside = tree.0.join("outside");
    fs::create_dir(&outside).unwrap();
    for file in 0..10 {
        fs::write(outside.join(file.to_string()), b"data").unwrap();
    }
    let link = root.join("link");
    junction::create(&outside, &link).unwrap();
    let cancellation = CancellationFlag::new();
    assert!(has_small_tree(&root, &cancellation, 2, Duration::from_secs(5)).unwrap());
    assert!(!has_small_tree(&root, &cancellation, 1, Duration::from_secs(5)).unwrap());
    assert!(!has_small_tree(&root, &cancellation, 100, Duration::ZERO).unwrap());
    assert!(
        !has_small_tree(
            std::path::Path::new(r"C:\"),
            &cancellation,
            100,
            Duration::from_secs(5)
        )
        .unwrap()
    );
    assert!(
        !has_small_tree(
            &tree.0.join("missing"),
            &cancellation,
            100,
            Duration::from_secs(5)
        )
        .unwrap()
    );
    cancellation.cancel();
    assert!(matches!(
        has_small_tree(&root, &cancellation, 100, Duration::from_secs(5)),
        Err(AppError::Cancelled)
    ));
    junction::delete(&link).unwrap();
}

#[test]
fn small_directory_scan_finishes_without_starting_mft() {
    let tree = TestTree::new();
    fs::write(tree.0.join("root-file"), b"hello").unwrap();
    fs::create_dir(tree.0.join("empty")).unwrap();
    let phases = std::sync::Mutex::new(Vec::new());
    let snapshot = scan_children_with_probe(
        &tree.0,
        &CancellationFlag::new(),
        &|update| {
            phases.lock().unwrap().push(update.phase);
        },
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(snapshot.engine, "快速遍历");
    assert_eq!(snapshot.total_files(), 1);
    assert_eq!(snapshot.total_bytes(), 5);
    assert!(
        !phases
            .lock()
            .unwrap()
            .iter()
            .any(|phase| phase.starts_with("mft"))
    );
}

#[test]
fn enumeration_handles_empty_flat_and_cancelled_roots() {
    let tree = TestTree::new();
    let cancellation = CancellationFlag::new();
    let empty = scan_children_by_enumeration(&tree.0, &cancellation, &|_| {}).unwrap();
    assert_eq!(empty.total_files(), 0);
    assert!(empty.folders.is_empty());
    fs::write(tree.0.join("file"), [2_u8; 5000]).unwrap();
    let flat = scan_children_by_enumeration(&tree.0, &cancellation, &|_| {}).unwrap();
    assert_eq!(flat.root_files, 1);
    assert_equivalent(
        flat,
        scan_children_by_enumeration_baseline(&tree.0, &cancellation, &|_| {}).unwrap(),
    );
    assert!(scan_children_by_enumeration(&tree.0.join("missing"), &cancellation, &|_| {}).is_err());
    let cancelled = scan_children_by_enumeration(&tree.0, &cancellation, &|_| {
        cancellation.cancel();
    });
    assert!(matches!(cancelled, Err(AppError::Cancelled)));
}

#[test]
#[ignore = "creates 10,000 temporary files to compare wide and single-root scans"]
fn benchmark_space_analysis() {
    let tree = TestTree::new();
    let outer = tree.0.join("single-large-folder");
    fs::create_dir(&outer).unwrap();
    for directory in 0..100 {
        let path = outer.join(format!("dir-{directory}"));
        fs::create_dir(&path).unwrap();
        for file in 0..100 {
            fs::write(path.join(format!("file-{file}.bin")), [1_u8; 128]).unwrap();
        }
    }
    let cancellation = CancellationFlag::new();
    for (label, root) in [
        ("100 top-level folders", &outer),
        ("one large top-level folder", &tree.0),
    ] {
        let mut old_times = Vec::new();
        let mut new_times = Vec::new();
        for round in 0..6 {
            let mut baseline = None;
            let mut optimized = None;
            for fast in [round % 2 == 0, round % 2 != 0] {
                let started = Instant::now();
                if fast {
                    let snapshot =
                        scan_children_by_enumeration(root, &cancellation, &|_| {}).unwrap();
                    new_times.push(started.elapsed());
                    optimized = Some(snapshot);
                } else {
                    let snapshot =
                        scan_children_by_enumeration_baseline(root, &cancellation, &|_| {})
                            .unwrap();
                    old_times.push(started.elapsed());
                    baseline = Some(snapshot);
                }
            }
            assert_equivalent(optimized.unwrap(), baseline.unwrap());
        }
        old_times.sort();
        new_times.sort();
        let old = old_times[old_times.len() / 2];
        let new = new_times[new_times.len() / 2];
        eprintln!(
            "Space analysis ({label}, 10,000 files): original={old:?}, optimized={new:?}, speedup={:.2}x",
            old.as_secs_f64() / new.as_secs_f64()
        );
    }
}

#[test]
#[ignore = "creates 10,000 temporary files for a comparative preflight benchmark"]
fn benchmark_migration_preflight() {
    let tree = TestTree::new();
    for directory in 0..100 {
        let path = tree.0.join(format!("dir-{directory}"));
        fs::create_dir(&path).unwrap();
        for file in 0..100 {
            fs::write(path.join(format!("file-{file}.bin")), [1_u8; 128]).unwrap();
        }
    }
    let cancellation = CancellationFlag::new();
    let mut full_times = Vec::new();
    let mut fast_times = Vec::new();
    // Alternate ordering so neither method always gets the warmer cache.
    for round in 0..6 {
        for fast in [round % 2 == 0, round % 2 != 0] {
            let start = Instant::now();
            if fast {
                let stats = measure_for_migration(&tree.0, &cancellation);
                fast_times.push(start.elapsed());
                assert_eq!(
                    stats,
                    MigrationStats {
                        bytes: 1_280_000,
                        files: 10_000,
                        links: 0,
                        errors: 0,
                    }
                );
            } else {
                let stats = measure(&tree.0, &cancellation);
                full_times.push(start.elapsed());
                assert_eq!(
                    (stats.bytes, stats.files, stats.links, stats.errors),
                    (1_280_000, 10_000, 0, 0)
                );
            }
        }
    }
    full_times.sort();
    fast_times.sort();
    let old = full_times[full_times.len() / 2];
    let new = fast_times[fast_times.len() / 2];
    eprintln!(
        "10,000-file preflight median: original={old:?}, optimized={new:?}, speedup={:.2}x",
        old.as_secs_f64() / new.as_secs_f64()
    );
}

// Kept only in tests as a reference for statistics and comparative benchmarks.
fn scan_children_by_enumeration_baseline(
    root: &Path,
    cancellation: &CancellationFlag,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> AppResult<ScanSnapshot> {
    let directories = list_child_paths(root)?;
    let root_stats = measure_direct_files(root, cancellation);
    let total = directories.len();
    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let rows = Mutex::new(Vec::with_capacity(total));
    let links = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .clamp(2, 8)
        .min(total.max(1));

    progress(ScanProgress {
        percentage: Some(5),
        phase: "fallback".to_string(),
        message: "正在使用兼容扫描…".to_string(),
    });

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    if cancellation.is_cancelled() {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = directories.get(index) else {
                        break;
                    };
                    let row = scan_top_folder(path, root, cancellation, &links);
                    if let Ok(mut target) = rows.lock() {
                        target.push(row);
                    }
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    let percentage = if total == 0 {
                        100
                    } else {
                        5 + (done * 94 / total).min(94)
                    };
                    progress(ScanProgress {
                        percentage: Some(u8::try_from(percentage).unwrap_or(99)),
                        phase: "fallback".to_string(),
                        message: format!("正在扫描文件夹 {done}/{total}"),
                    });
                }
            });
        }
    });
    cancellation.check()?;

    let rows = rows
        .into_inner()
        .map_err(|_| AppError::Task("扫描结果锁已损坏。".to_string()))?;
    let links = links
        .into_inner()
        .map_err(|_| AppError::Task("链接结果锁已损坏。".to_string()))?;
    progress(ScanProgress {
        percentage: Some(100),
        phase: "complete".to_string(),
        message: "扫描完成".to_string(),
    });
    Ok(ScanSnapshot {
        id: Uuid::new_v4(),
        root: root.to_string_lossy().into_owned(),
        folders: rows,
        links,
        root_bytes: root_stats.bytes,
        root_allocated_bytes: root_stats.allocated_bytes,
        root_files: root_stats.files,
        engine: "兼容模式".to_string(),
        created_at_utc: Utc::now(),
    })
}

fn measure(root: &Path, cancellation: &CancellationFlag) -> FolderStats {
    measure_with(root, cancellation, |entry| measure_entry(&entry.path()))
}

fn measure_direct_files(root: &Path, cancellation: &CancellationFlag) -> FolderStats {
    let mut stats = FolderStats::default();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => {
            stats.errors = 1;
            return stats;
        }
    };
    for entry in entries {
        if cancellation.is_cancelled() {
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                stats.errors = stats.errors.saturating_add(1);
                continue;
            }
        };
        match measure_entry(&entry.path()) {
            Ok(MeasureEntry::File {
                bytes,
                allocated_bytes,
                is_link,
            }) => {
                stats.bytes = stats.bytes.saturating_add(bytes);
                stats.allocated_bytes = stats.allocated_bytes.saturating_add(allocated_bytes);
                stats.files = stats.files.saturating_add(1);
                stats.links = stats.links.saturating_add(u64::from(is_link));
            }
            Ok(MeasureEntry::Link) => stats.links = stats.links.saturating_add(1),
            Ok(MeasureEntry::Directory) => {}
            Err(_) => stats.errors = stats.errors.saturating_add(1),
        }
    }
    stats
}

fn list_child_paths(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        let is_directory = fs::symlink_metadata(&path)
            .map(|metadata| metadata.is_dir() || metadata.file_attributes() & 0x10 != 0)
            .unwrap_or(false);
        if is_directory {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn scan_top_folder(
    path: &Path,
    scan_root: &Path,
    cancellation: &CancellationFlag,
    links: &Mutex<Vec<LinkRow>>,
) -> FolderRow {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_link = platform::is_reparse_point(path).unwrap_or(false);
    if is_link {
        let link = create_link_row(path, true, scan_root);
        if let Ok(mut target) = links.lock() {
            target.push(link.clone());
        }
        return FolderRow {
            name,
            full_path: path.to_string_lossy().into_owned(),
            bytes: 0,
            allocated_bytes: 0,
            file_count: 0,
            link_count: 1,
            error_count: 0,
            is_link: true,
            link_target: (link.target != "无法解析").then_some(link.target),
            note: "兼容扫描：目录链接".to_string(),
            measured: true,
        };
    }

    let stats = measure(path, cancellation);
    FolderRow {
        name,
        full_path: path.to_string_lossy().into_owned(),
        bytes: stats.bytes,
        allocated_bytes: stats.allocated_bytes,
        file_count: stats.files,
        link_count: stats.links,
        error_count: stats.errors,
        is_link: false,
        link_target: None,
        note: if stats.errors == 0 {
            "兼容扫描".to_string()
        } else {
            format!("兼容扫描：{} 项无法读取", stats.errors)
        },
        measured: true,
    }
}

fn measure_entry(path: &Path) -> AppResult<MeasureEntry> {
    let metadata = fs::symlink_metadata(path)?;
    Ok(classify_entry(&metadata, || {
        platform::allocated_size(path).unwrap_or(metadata.len())
    }))
}

#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive tests for the workspace staging layer (abp-workspace).

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog, OperationSummary};
use abp_workspace::snapshot::{self, FileSnapshot, SnapshotDiff, WorkspaceSnapshot};
use abp_workspace::template::{TemplateRegistry, WorkspaceTemplate};
use abp_workspace::tracker::{ChangeKind, ChangeSummary, ChangeTracker, FileChange};
use abp_workspace::{PreparedWorkspace, WorkspaceManager, WorkspaceStager};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ── helpers ────────────────────────────────────────────────────────────

fn make_source_tree(files: &[(&str, &str)]) -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for (rel, content) in files {
        let p = tmp.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
    }
    tmp
}

fn file_exists(base: &Path, rel: &str) -> bool {
    base.join(rel).exists()
}

fn read_file(base: &Path, rel: &str) -> String {
    fs::read_to_string(base.join(rel)).unwrap()
}

fn count_files(dir: &Path) -> usize {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .count()
}

fn dir_exists(base: &Path, rel: &str) -> bool {
    base.join(rel).is_dir()
}

// ═══════════════════════════════════════════════════════════════════════
// Section 1: WorkspaceManager – PassThrough mode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_returns_original_path() {
    let src = make_source_tree(&[("a.txt", "hello")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn passthrough_no_copy_created() {
    let src = make_source_tree(&[("a.txt", "data")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(read_file(ws.path(), "a.txt"), "data");
}

#[test]
fn passthrough_does_not_init_git() {
    let src = make_source_tree(&[("a.txt", "x")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    // .git should NOT be created in passthrough mode
    assert!(!ws.path().join(".git").exists());
}

// ═══════════════════════════════════════════════════════════════════════
// Section 2: WorkspaceManager – Staged mode basics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_creates_different_path() {
    let src = make_source_tree(&[("a.txt", "hello")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn staged_copies_single_file() {
    let src = make_source_tree(&[("hello.txt", "world")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(read_file(ws.path(), "hello.txt"), "world");
}

#[test]
fn staged_copies_multiple_files() {
    let src = make_source_tree(&[("a.txt", "A"), ("b.txt", "B"), ("c.txt", "C")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(read_file(ws.path(), "a.txt"), "A");
    assert_eq!(read_file(ws.path(), "b.txt"), "B");
    assert_eq!(read_file(ws.path(), "c.txt"), "C");
}

#[test]
fn staged_preserves_file_content_exactly() {
    let content = "line1\nline2\nline3\n\ttabbed\n";
    let src = make_source_tree(&[("data.txt", content)]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(read_file(ws.path(), "data.txt"), content);
}

#[test]
fn staged_inits_git_repo() {
    let src = make_source_tree(&[("f.txt", "x")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join(".git").exists());
}

// ═══════════════════════════════════════════════════════════════════════
// Section 3: .git directory exclusion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_excludes_source_dot_git() {
    let src = make_source_tree(&[("a.txt", "a")]);
    // Manually create a fake .git directory with content in source
    fs::create_dir_all(src.path().join(".git/objects")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    // The staged .git should be from git init, not copied from source
    assert!(ws.path().join(".git").exists());
    assert!(file_exists(ws.path(), "a.txt"));
    // The source .git/objects should NOT be in the staged workspace alongside the new .git
    // The staged repo is initialized fresh
}

#[test]
fn staged_does_not_copy_dot_git_contents() {
    let src = make_source_tree(&[("readme.md", "# Hello")]);
    fs::create_dir_all(src.path().join(".git/refs")).unwrap();
    fs::write(src.path().join(".git/marker"), "should_not_copy").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    // The marker file from source .git should not be present
    assert!(!file_exists(ws.path(), ".git/marker"));
}

// ═══════════════════════════════════════════════════════════════════════
// Section 4: Include/Exclude glob patterns (via WorkspaceManager)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_exclude_glob_filters_files() {
    let src = make_source_tree(&[("a.txt", "A"), ("b.log", "B"), ("c.txt", "C")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["*.log".into()],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "a.txt"));
    assert!(!file_exists(ws.path(), "b.log"));
    assert!(file_exists(ws.path(), "c.txt"));
}

#[test]
fn staged_include_glob_limits_files() {
    let src = make_source_tree(&[
        ("a.rs", "fn main(){}"),
        ("b.txt", "text"),
        ("c.rs", "fn f(){}"),
    ]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "a.rs"));
    assert!(!file_exists(ws.path(), "b.txt"));
    assert!(file_exists(ws.path(), "c.rs"));
}

#[test]
fn staged_exclude_overrides_include() {
    let src = make_source_tree(&[
        ("src/main.rs", "fn main(){}"),
        ("src/test.rs", "fn test(){}"),
        ("src/lib.rs", "pub mod lib;"),
    ]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/*.rs".into()],
        exclude: vec!["**/test.rs".into()],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(!file_exists(ws.path(), "src/test.rs"));
    assert!(file_exists(ws.path(), "src/lib.rs"));
}

#[test]
fn staged_exclude_multiple_patterns() {
    let src = make_source_tree(&[
        ("a.txt", "A"),
        ("b.log", "B"),
        ("c.tmp", "C"),
        ("d.rs", "D"),
    ]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["*.log".into(), "*.tmp".into()],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "a.txt"));
    assert!(!file_exists(ws.path(), "b.log"));
    assert!(!file_exists(ws.path(), "c.tmp"));
    assert!(file_exists(ws.path(), "d.rs"));
}

#[test]
fn staged_empty_include_exclude_copies_all() {
    let src = make_source_tree(&[("a.txt", "A"), ("b.txt", "B")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(count_files(ws.path()), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Section 5: Nested directory copying
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_copies_nested_directories() {
    let src = make_source_tree(&[
        ("src/main.rs", "fn main(){}"),
        ("src/lib/mod.rs", "pub mod x;"),
        ("src/lib/x.rs", "pub fn x(){}"),
    ]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(file_exists(ws.path(), "src/lib/mod.rs"));
    assert!(file_exists(ws.path(), "src/lib/x.rs"));
}

#[test]
fn staged_deeply_nested_five_levels() {
    let src = make_source_tree(&[("a/b/c/d/e/deep.txt", "deep content")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(read_file(ws.path(), "a/b/c/d/e/deep.txt"), "deep content");
}

#[test]
fn staged_preserves_directory_structure() {
    let src = make_source_tree(&[
        ("dir1/file1.txt", "1"),
        ("dir2/file2.txt", "2"),
        ("dir1/sub/file3.txt", "3"),
    ]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(dir_exists(ws.path(), "dir1"));
    assert!(dir_exists(ws.path(), "dir2"));
    assert!(dir_exists(ws.path(), "dir1/sub"));
}

// ═══════════════════════════════════════════════════════════════════════
// Section 6: Empty directory handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_empty_source_produces_empty_workspace() {
    let src = tempfile::tempdir().unwrap();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(count_files(ws.path()), 0);
}

#[test]
fn staged_source_with_only_empty_dirs() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_a")).unwrap();
    fs::create_dir_all(src.path().join("empty_b")).unwrap();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(count_files(ws.path()), 0);
    // Empty dirs should be copied
    assert!(dir_exists(ws.path(), "empty_a"));
    assert!(dir_exists(ws.path(), "empty_b"));
}

// ═══════════════════════════════════════════════════════════════════════
// Section 7: Large file handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_copies_large_file() {
    let src = tempfile::tempdir().unwrap();
    let large = "x".repeat(1_000_000); // 1MB
    fs::write(src.path().join("big.bin"), &large).unwrap();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let copied = fs::read_to_string(ws.path().join("big.bin")).unwrap();
    assert_eq!(copied.len(), 1_000_000);
    assert_eq!(copied, large);
}

#[test]
fn staged_binary_file_preserved() {
    let src = tempfile::tempdir().unwrap();
    let binary: Vec<u8> = (0..=255u8).collect();
    fs::write(src.path().join("binary.dat"), &binary).unwrap();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let copied = fs::read(ws.path().join("binary.dat")).unwrap();
    assert_eq!(copied, binary);
}

#[test]
fn staged_empty_file_preserved() {
    let src = make_source_tree(&[("empty.txt", "")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(read_file(ws.path(), "empty.txt"), "");
}

// ═══════════════════════════════════════════════════════════════════════
// Section 8: Workspace cleanup on drop
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_workspace_cleaned_up_on_drop() {
    let src = make_source_tree(&[("a.txt", "A")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let staged_path = ws.path().to_path_buf();
    assert!(staged_path.exists());
    drop(ws);
    assert!(!staged_path.exists());
}

#[test]
fn passthrough_workspace_not_cleaned_on_drop() {
    let src = make_source_tree(&[("a.txt", "A")]);
    let src_path = src.path().to_path_buf();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    drop(ws);
    assert!(src_path.exists());
}

// ═══════════════════════════════════════════════════════════════════════
// Section 9: WorkspaceStager builder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stager_basic_stage() {
    let src = make_source_tree(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "file.txt"), "content");
}

#[test]
fn stager_default_enables_git_init() {
    let src = make_source_tree(&[("a.txt", "a")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_git_init_disabled() {
    let src = make_source_tree(&[("a.txt", "a")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn stager_exclude_patterns() {
    let src = make_source_tree(&[("a.rs", "fn()"), ("b.log", "log"), ("c.rs", "fn2()")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a.rs"));
    assert!(!file_exists(ws.path(), "b.log"));
    assert!(file_exists(ws.path(), "c.rs"));
}

#[test]
fn stager_include_patterns() {
    let src = make_source_tree(&[("a.rs", "fn()"), ("b.txt", "text")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a.rs"));
    assert!(!file_exists(ws.path(), "b.txt"));
}

#[test]
fn stager_requires_source_root() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
}

#[test]
fn stager_nonexistent_source_errors() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/abp_test_xyz")
        .stage();
    assert!(result.is_err());
}

#[test]
fn stager_cleaned_on_drop() {
    let src = make_source_tree(&[("a.txt", "A")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let p = ws.path().to_path_buf();
    assert!(p.exists());
    drop(ws);
    assert!(!p.exists());
}

#[test]
fn stager_copies_nested_dirs() {
    let src = make_source_tree(&[("x/y/z.txt", "nested"), ("x/a.txt", "a")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "x/y/z.txt"), "nested");
    assert_eq!(read_file(ws.path(), "x/a.txt"), "a");
}

#[test]
fn stager_include_and_exclude_combined() {
    let src = make_source_tree(&[
        ("src/main.rs", "main"),
        ("src/test.rs", "test"),
        ("docs/readme.md", "readme"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/*.rs".into()])
        .exclude(vec!["**/test.rs".into()])
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(!file_exists(ws.path(), "src/test.rs"));
    assert!(!file_exists(ws.path(), "docs/readme.md"));
}

#[test]
fn stager_chained_builder_methods() {
    let src = make_source_tree(&[("a.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec![])
        .exclude(vec![])
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a.txt"));
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_default_is_new() {
    let stager = WorkspaceStager::default();
    // Default should be equivalent to new()
    let src = make_source_tree(&[("a.txt", "a")]);
    let ws = stager.source_root(src.path()).stage().unwrap();
    assert!(file_exists(ws.path(), "a.txt"));
}

// ═══════════════════════════════════════════════════════════════════════
// Section 10: Path accessors
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn path_accessor_returns_valid_path() {
    let src = make_source_tree(&[("a.txt", "A")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().is_dir());
}

#[test]
fn path_accessor_is_absolute() {
    let src = make_source_tree(&[("a.txt", "A")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().is_absolute());
}

// ═══════════════════════════════════════════════════════════════════════
// Section 11: Git initialisation in staged workspace
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_git_repo_has_baseline_commit() {
    let src = make_source_tree(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    // Verify git log contains a baseline commit
    let output = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(
        log.contains("baseline"),
        "expected baseline commit, got: {log}"
    );
}

#[test]
fn staged_git_status_clean_after_init() {
    let src = make_source_tree(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    // After baseline commit, status should be clean (empty)
    assert!(
        status.as_ref().map_or(true, |s| s.trim().is_empty()),
        "expected clean status, got: {:?}",
        status
    );
}

#[test]
fn staged_git_diff_empty_initially() {
    let src = make_source_tree(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(
        diff.as_ref().map_or(true, |d| d.trim().is_empty()),
        "expected empty diff, got: {:?}",
        diff
    );
}

#[test]
fn staged_git_diff_after_modification() {
    let src = make_source_tree(&[("f.txt", "original")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    // Modify a file in staged workspace
    fs::write(ws.path().join("f.txt"), "modified").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    let diff = diff.unwrap();
    assert!(diff.contains("original") || diff.contains("modified"));
}

#[test]
fn staged_git_status_after_new_file() {
    let src = make_source_tree(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("new.txt"), "new content").unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(status.unwrap().contains("new.txt"));
}

#[test]
fn staged_no_git_when_disabled() {
    let src = make_source_tree(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

// ═══════════════════════════════════════════════════════════════════════
// Section 12: Snapshot capture & compare
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_captures_files() {
    let src = make_source_tree(&[("a.txt", "hello"), ("b.txt", "world")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 2);
}

#[test]
fn snapshot_records_file_size() {
    let src = make_source_tree(&[("a.txt", "12345")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.total_size(), 5);
}

#[test]
fn snapshot_has_file_check() {
    let src = make_source_tree(&[("a.txt", "x"), ("b.txt", "y")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.has_file("a.txt"));
    assert!(snap.has_file("b.txt"));
    assert!(!snap.has_file("c.txt"));
}

#[test]
fn snapshot_get_file_returns_metadata() {
    let src = make_source_tree(&[("a.txt", "hello")]);
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file("a.txt").unwrap();
    assert_eq!(f.size, 5);
    assert!(!f.is_binary);
    assert!(!f.sha256.is_empty());
}

#[test]
fn snapshot_binary_detection() {
    let src = tempfile::tempdir().unwrap();
    let mut data = vec![0u8; 100];
    data[0] = 0; // null byte -> binary
    fs::write(src.path().join("bin.dat"), &data).unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file("bin.dat").unwrap();
    assert!(f.is_binary);
}

#[test]
fn snapshot_text_not_binary() {
    let src = make_source_tree(&[("text.txt", "hello world\n")]);
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file("text.txt").unwrap();
    assert!(!f.is_binary);
}

#[test]
fn snapshot_excludes_git_dir() {
    let src = make_source_tree(&[("a.txt", "x")]);
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref").unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(!snap.has_file(".git/HEAD"));
    assert_eq!(snap.file_count(), 1);
}

#[test]
fn snapshot_compare_identical() {
    let src = make_source_tree(&[("a.txt", "hello")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 1);
}

#[test]
fn snapshot_compare_added_file() {
    let src = make_source_tree(&[("a.txt", "a")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert_eq!(diff.added.len(), 1);
    assert_eq!(diff.added[0], PathBuf::from("b.txt"));
}

#[test]
fn snapshot_compare_removed_file() {
    let src = make_source_tree(&[("a.txt", "a"), ("b.txt", "b")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    fs::remove_file(src.path().join("b.txt")).unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert_eq!(diff.removed.len(), 1);
    assert_eq!(diff.removed[0], PathBuf::from("b.txt"));
}

#[test]
fn snapshot_compare_modified_file() {
    let src = make_source_tree(&[("a.txt", "original")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("a.txt"), "changed").unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert_eq!(diff.modified.len(), 1);
    assert_eq!(diff.modified[0], PathBuf::from("a.txt"));
}

#[test]
fn snapshot_compare_empty_snapshots() {
    let s1 = tempfile::tempdir().unwrap();
    let s2 = tempfile::tempdir().unwrap();
    let snap1 = snapshot::capture(s1.path()).unwrap();
    let snap2 = snapshot::capture(s2.path()).unwrap();
    let diff = snapshot::compare(&snap1, &snap2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert!(diff.unchanged.is_empty());
}

#[test]
fn snapshot_sha256_deterministic() {
    let src = make_source_tree(&[("a.txt", "deterministic")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let h1 = &s1.get_file("a.txt").unwrap().sha256;
    let h2 = &s2.get_file("a.txt").unwrap().sha256;
    assert_eq!(h1, h2);
}

#[test]
fn snapshot_different_content_different_hash() {
    let d1 = make_source_tree(&[("a.txt", "content_a")]);
    let d2 = make_source_tree(&[("a.txt", "content_b")]);
    let s1 = snapshot::capture(d1.path()).unwrap();
    let s2 = snapshot::capture(d2.path()).unwrap();
    assert_ne!(
        s1.get_file("a.txt").unwrap().sha256,
        s2.get_file("a.txt").unwrap().sha256
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Section 13: OperationLog and FileOperation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn operation_log_empty_by_default() {
    let log = OperationLog::new();
    assert!(log.operations().is_empty());
}

#[test]
fn operation_log_record_read() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    assert_eq!(log.reads(), vec!["a.txt"]);
}

#[test]
fn operation_log_record_write() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Write {
        path: "b.txt".into(),
        size: 42,
    });
    assert_eq!(log.writes(), vec!["b.txt"]);
}

#[test]
fn operation_log_record_delete() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Delete {
        path: "c.txt".into(),
    });
    assert_eq!(log.deletes(), vec!["c.txt"]);
}

#[test]
fn operation_log_record_move() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Move {
        from: "old.txt".into(),
        to: "new.txt".into(),
    });
    let paths = log.affected_paths();
    assert!(paths.contains("old.txt"));
    assert!(paths.contains("new.txt"));
}

#[test]
fn operation_log_record_copy() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Copy {
        from: "src.txt".into(),
        to: "dst.txt".into(),
    });
    let paths = log.affected_paths();
    assert!(paths.contains("src.txt"));
    assert!(paths.contains("dst.txt"));
}

#[test]
fn operation_log_record_create_dir() {
    let mut log = OperationLog::new();
    log.record(FileOperation::CreateDir {
        path: "new_dir".into(),
    });
    let paths = log.affected_paths();
    assert!(paths.contains("new_dir"));
}

#[test]
fn operation_log_summary_counts() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read { path: "a".into() });
    log.record(FileOperation::Read { path: "b".into() });
    log.record(FileOperation::Write {
        path: "c".into(),
        size: 10,
    });
    log.record(FileOperation::Delete { path: "d".into() });
    log.record(FileOperation::Move {
        from: "e".into(),
        to: "f".into(),
    });
    log.record(FileOperation::Copy {
        from: "g".into(),
        to: "h".into(),
    });
    log.record(FileOperation::CreateDir { path: "i".into() });
    let s = log.summary();
    assert_eq!(s.reads, 2);
    assert_eq!(s.writes, 1);
    assert_eq!(s.deletes, 1);
    assert_eq!(s.moves, 1);
    assert_eq!(s.copies, 1);
    assert_eq!(s.create_dirs, 1);
    assert_eq!(s.total_writes_bytes, 10);
}

#[test]
fn operation_log_summary_total_bytes() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Write {
        path: "a".into(),
        size: 100,
    });
    log.record(FileOperation::Write {
        path: "b".into(),
        size: 200,
    });
    assert_eq!(log.summary().total_writes_bytes, 300);
}

#[test]
fn operation_log_clear() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read { path: "a".into() });
    log.clear();
    assert!(log.operations().is_empty());
}

#[test]
fn operation_log_affected_paths_unique() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "a.txt".into(),
        size: 5,
    });
    let paths = log.affected_paths();
    assert_eq!(paths.len(), 1);
}

#[test]
fn file_operation_paths_read() {
    let op = FileOperation::Read {
        path: "x.txt".into(),
    };
    assert_eq!(op.paths(), vec!["x.txt"]);
}

#[test]
fn file_operation_paths_write() {
    let op = FileOperation::Write {
        path: "x.txt".into(),
        size: 0,
    };
    assert_eq!(op.paths(), vec!["x.txt"]);
}

#[test]
fn file_operation_paths_move() {
    let op = FileOperation::Move {
        from: "a".into(),
        to: "b".into(),
    };
    assert_eq!(op.paths(), vec!["a", "b"]);
}

#[test]
fn file_operation_paths_copy() {
    let op = FileOperation::Copy {
        from: "a".into(),
        to: "b".into(),
    };
    assert_eq!(op.paths(), vec!["a", "b"]);
}

// ═══════════════════════════════════════════════════════════════════════
// Section 14: OperationFilter
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn filter_default_allows_all() {
    let filter = OperationFilter::new();
    assert!(filter.is_allowed("anything.txt"));
}

#[test]
fn filter_denied_path_blocks() {
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.log");
    assert!(!filter.is_allowed("debug.log"));
    assert!(filter.is_allowed("main.rs"));
}

#[test]
fn filter_allowed_path_restricts() {
    let mut filter = OperationFilter::new();
    filter.add_allowed_path("*.rs");
    assert!(filter.is_allowed("main.rs"));
    assert!(!filter.is_allowed("data.txt"));
}

#[test]
fn filter_deny_overrides_allow() {
    let mut filter = OperationFilter::new();
    filter.add_allowed_path("*.rs");
    filter.add_denied_path("test.rs");
    assert!(filter.is_allowed("main.rs"));
    assert!(!filter.is_allowed("test.rs"));
}

#[test]
fn filter_operations_filters_ops() {
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.log");
    let ops = vec![
        FileOperation::Read {
            path: "main.rs".into(),
        },
        FileOperation::Read {
            path: "debug.log".into(),
        },
    ];
    let filtered = filter.filter_operations(&ops);
    assert_eq!(filtered.len(), 1);
}

#[test]
fn filter_operations_move_both_paths_checked() {
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.log");
    let ops = vec![FileOperation::Move {
        from: "clean.txt".into(),
        to: "debug.log".into(),
    }];
    let filtered = filter.filter_operations(&ops);
    assert_eq!(filtered.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// Section 15: ChangeTracker
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn change_tracker_empty_by_default() {
    let tracker = ChangeTracker::new();
    assert!(!tracker.has_changes());
    assert!(tracker.changes().is_empty());
}

#[test]
fn change_tracker_record_created() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "new.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    assert!(tracker.has_changes());
    assert_eq!(tracker.summary().created, 1);
}

#[test]
fn change_tracker_record_modified() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "mod.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(50),
        size_after: Some(100),
        content_hash: None,
    });
    assert_eq!(tracker.summary().modified, 1);
}

#[test]
fn change_tracker_record_deleted() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "del.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(100),
        size_after: None,
        content_hash: None,
    });
    assert_eq!(tracker.summary().deleted, 1);
}

#[test]
fn change_tracker_record_renamed() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "new_name.txt".into(),
        kind: ChangeKind::Renamed {
            from: "old_name.txt".into(),
        },
        size_before: Some(50),
        size_after: Some(50),
        content_hash: None,
    });
    assert_eq!(tracker.summary().renamed, 1);
}

#[test]
fn change_tracker_size_delta_positive() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "grow.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(10),
        size_after: Some(50),
        content_hash: None,
    });
    assert_eq!(tracker.summary().total_size_delta, 40);
}

#[test]
fn change_tracker_size_delta_negative() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "shrink.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(100),
        size_after: Some(20),
        content_hash: None,
    });
    assert_eq!(tracker.summary().total_size_delta, -80);
}

#[test]
fn change_tracker_size_delta_across_multiple() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(30),
        size_after: None,
        content_hash: None,
    });
    // created: 0->100 = +100, deleted: 30->0 = -30, net = +70
    assert_eq!(tracker.summary().total_size_delta, 70);
}

#[test]
fn change_tracker_by_kind() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(5),
        size_after: Some(15),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(20),
        content_hash: None,
    });
    let created = tracker.by_kind(&ChangeKind::Created);
    assert_eq!(created.len(), 2);
}

#[test]
fn change_tracker_affected_paths() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(5),
        size_after: Some(15),
        content_hash: None,
    });
    let paths = tracker.affected_paths();
    assert_eq!(paths, vec!["a.txt", "b.txt"]);
}

#[test]
fn change_tracker_affected_paths_dedup() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(10),
        size_after: Some(20),
        content_hash: None,
    });
    let paths = tracker.affected_paths();
    assert_eq!(paths, vec!["a.txt"]);
}

#[test]
fn change_tracker_clear() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    tracker.clear();
    assert!(!tracker.has_changes());
}

// ═══════════════════════════════════════════════════════════════════════
// Section 16: WorkspaceTemplate
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn template_new_empty() {
    let t = WorkspaceTemplate::new("test", "A test template");
    assert_eq!(t.name, "test");
    assert_eq!(t.description, "A test template");
    assert_eq!(t.file_count(), 0);
}

#[test]
fn template_add_file() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("hello.txt", "world");
    assert_eq!(t.file_count(), 1);
    assert!(t.has_file("hello.txt"));
}

#[test]
fn template_apply_creates_files() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("src/main.rs", "fn main(){}");
    t.add_file("README.md", "# Hello");
    let dir = tempfile::tempdir().unwrap();
    let written = t.apply(dir.path()).unwrap();
    assert_eq!(written, 2);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
        "fn main(){}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "# Hello"
    );
}

#[test]
fn template_apply_creates_parent_dirs() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("a/b/c/deep.txt", "content");
    let dir = tempfile::tempdir().unwrap();
    t.apply(dir.path()).unwrap();
    assert!(dir.path().join("a/b/c/deep.txt").exists());
}

#[test]
fn template_validate_valid() {
    let t = WorkspaceTemplate::new("name", "desc");
    assert!(t.validate().is_empty());
}

#[test]
fn template_validate_empty_name() {
    let t = WorkspaceTemplate::new("", "desc");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("name")));
}

#[test]
fn template_validate_empty_description() {
    let t = WorkspaceTemplate::new("name", "");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("description")));
}

#[test]
fn template_validate_absolute_path() {
    let mut t = WorkspaceTemplate::new("name", "desc");
    #[cfg(windows)]
    {
        t.files
            .insert(PathBuf::from("C:\\absolute\\path.txt"), "data".into());
    }
    #[cfg(not(windows))]
    {
        t.files
            .insert(PathBuf::from("/absolute/path.txt"), "data".into());
    }
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("absolute")));
}

#[test]
fn template_has_file_false_for_missing() {
    let t = WorkspaceTemplate::new("t", "d");
    assert!(!t.has_file("nonexistent.txt"));
}

// ═══════════════════════════════════════════════════════════════════════
// Section 17: TemplateRegistry
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_empty_by_default() {
    let reg = TemplateRegistry::new();
    assert_eq!(reg.count(), 0);
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("foo", "Foo template"));
    assert_eq!(reg.count(), 1);
    assert!(reg.get("foo").is_some());
    assert_eq!(reg.get("foo").unwrap().name, "foo");
}

#[test]
fn registry_get_missing_returns_none() {
    let reg = TemplateRegistry::new();
    assert!(reg.get("missing").is_none());
}

#[test]
fn registry_list_sorted() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("beta", "b"));
    reg.register(WorkspaceTemplate::new("alpha", "a"));
    reg.register(WorkspaceTemplate::new("gamma", "g"));
    assert_eq!(reg.list(), vec!["alpha", "beta", "gamma"]);
}

#[test]
fn registry_overwrite_existing() {
    let mut reg = TemplateRegistry::new();
    let mut t1 = WorkspaceTemplate::new("foo", "v1");
    t1.add_file("a.txt", "v1");
    reg.register(t1);

    let mut t2 = WorkspaceTemplate::new("foo", "v2");
    t2.add_file("b.txt", "v2");
    reg.register(t2);

    assert_eq!(reg.count(), 1);
    let t = reg.get("foo").unwrap();
    assert_eq!(t.description, "v2");
    assert!(t.has_file("b.txt"));
    assert!(!t.has_file("a.txt"));
}

// ═══════════════════════════════════════════════════════════════════════
// Section 18: Edge cases – special filenames & content
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn staged_file_with_spaces_in_name() {
    let src = make_source_tree(&[("my file.txt", "space content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "my file.txt"), "space content");
}

#[test]
fn staged_file_with_unicode_content() {
    let content = "こんにちは世界 🌍 émojis";
    let src = make_source_tree(&[("unicode.txt", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "unicode.txt"), content);
}

#[test]
fn staged_file_with_newlines_only() {
    let content = "\n\n\n\n";
    let src = make_source_tree(&[("newlines.txt", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "newlines.txt"), content);
}

#[test]
fn staged_many_files() {
    let src = tempfile::tempdir().unwrap();
    for i in 0..50 {
        fs::write(
            src.path().join(format!("file_{i}.txt")),
            format!("content_{i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(count_files(ws.path()), 50);
}

#[test]
fn staged_dotfile_copied() {
    let src = make_source_tree(&[(".hidden", "secret"), ("visible.txt", "public")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), ".hidden"));
    assert_eq!(read_file(ws.path(), ".hidden"), "secret");
}

#[test]
fn staged_exclude_dotfiles_via_glob() {
    let src = make_source_tree(&[
        (".hidden", "secret"),
        (".config", "cfg"),
        ("visible.txt", "public"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec![".*".into()])
        .stage()
        .unwrap();
    assert!(!file_exists(ws.path(), ".hidden"));
    assert!(!file_exists(ws.path(), ".config"));
    assert!(file_exists(ws.path(), "visible.txt"));
}

#[test]
fn staged_exclude_directory_pattern() {
    let src = make_source_tree(&[
        ("src/main.rs", "main"),
        ("target/debug/bin", "binary"),
        ("target/release/bin", "binary"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["target/**".into()])
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(!file_exists(ws.path(), "target/debug/bin"));
    assert!(!file_exists(ws.path(), "target/release/bin"));
}

#[test]
fn staged_include_only_specific_extension() {
    let src = make_source_tree(&[
        ("a.py", "print('hi')"),
        ("b.rs", "fn main(){}"),
        ("c.js", "console.log('hi')"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.py".into()])
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a.py"));
    assert!(!file_exists(ws.path(), "b.rs"));
    assert!(!file_exists(ws.path(), "c.js"));
}

// ═══════════════════════════════════════════════════════════════════════
// Section 19: Snapshot integration with staging
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_staged_workspace_matches_source() {
    let src = make_source_tree(&[("a.txt", "hello"), ("b.txt", "world")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let src_snap = snapshot::capture(src.path()).unwrap();
    let ws_snap = snapshot::capture(ws.path()).unwrap();
    let diff = snapshot::compare(&src_snap, &ws_snap);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 2);
}

#[test]
fn snapshot_staged_with_exclusions() {
    let src = make_source_tree(&[("a.txt", "A"), ("b.log", "B")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let ws_snap = snapshot::capture(ws.path()).unwrap();
    assert!(ws_snap.has_file("a.txt"));
    assert!(!ws_snap.has_file("b.log"));
}

// ═══════════════════════════════════════════════════════════════════════
// Section 20: More edge cases and combined tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn operation_summary_default_zeros() {
    let s = OperationSummary::default();
    assert_eq!(s.reads, 0);
    assert_eq!(s.writes, 0);
    assert_eq!(s.deletes, 0);
    assert_eq!(s.moves, 0);
    assert_eq!(s.copies, 0);
    assert_eq!(s.create_dirs, 0);
    assert_eq!(s.total_writes_bytes, 0);
}

#[test]
fn change_summary_default_zeros() {
    let s = ChangeSummary::default();
    assert_eq!(s.created, 0);
    assert_eq!(s.modified, 0);
    assert_eq!(s.deleted, 0);
    assert_eq!(s.renamed, 0);
    assert_eq!(s.total_size_delta, 0);
}

#[test]
fn staged_workspace_is_independent_of_source() {
    let src = make_source_tree(&[("a.txt", "original")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Modify source after staging
    fs::write(src.path().join("a.txt"), "modified in source").unwrap();
    // Staged workspace should still have original content
    assert_eq!(read_file(ws.path(), "a.txt"), "original");
}

#[test]
fn staged_modifications_dont_affect_source() {
    let src = make_source_tree(&[("a.txt", "original")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    fs::write(ws.path().join("a.txt"), "modified in staged").unwrap();
    assert_eq!(read_file(src.path(), "a.txt"), "original");
}

#[test]
fn staged_multiple_exclusions_complex() {
    let src = make_source_tree(&[
        ("src/main.rs", "main"),
        ("src/lib.rs", "lib"),
        ("tests/test.rs", "test"),
        ("build/output.bin", "bin"),
        ("docs/readme.md", "docs"),
        ("node_modules/pkg/index.js", "js"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["build/**".into(), "node_modules/**".into()])
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(file_exists(ws.path(), "tests/test.rs"));
    assert!(!file_exists(ws.path(), "build/output.bin"));
    assert!(file_exists(ws.path(), "docs/readme.md"));
    assert!(!file_exists(ws.path(), "node_modules/pkg/index.js"));
}

#[test]
fn snapshot_nested_files_counted() {
    let src = make_source_tree(&[("a/b.txt", "1"), ("a/c/d.txt", "2"), ("e.txt", "3")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 3);
}

#[test]
fn snapshot_total_size_across_nested() {
    let src = make_source_tree(&[("a.txt", "12345"), ("b/c.txt", "67890")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.total_size(), 10);
}

#[test]
fn operation_log_insertion_order_preserved() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "first".into(),
    });
    log.record(FileOperation::Read {
        path: "second".into(),
    });
    log.record(FileOperation::Read {
        path: "third".into(),
    });
    assert_eq!(log.reads(), vec!["first", "second", "third"]);
}

#[test]
fn stager_with_git_init_true_explicitly() {
    let src = make_source_tree(&[("a.txt", "a")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn staged_file_size_preserved() {
    let content = "exactly 26 bytes of data!\n";
    assert_eq!(content.len(), 26);
    let src = make_source_tree(&[("sized.txt", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let meta = fs::metadata(ws.path().join("sized.txt")).unwrap();
    assert_eq!(meta.len(), 26);
}

#[test]
fn snapshot_compare_multiple_changes() {
    let src = make_source_tree(&[("a.txt", "a"), ("b.txt", "b"), ("c.txt", "c")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    // Remove b, modify a, add d
    fs::remove_file(src.path().join("b.txt")).unwrap();
    fs::write(src.path().join("a.txt"), "a_modified").unwrap();
    fs::write(src.path().join("d.txt"), "d").unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert_eq!(diff.removed.len(), 1);
    assert_eq!(diff.modified.len(), 1);
    assert_eq!(diff.added.len(), 1);
    assert_eq!(diff.unchanged.len(), 1);
}

#[test]
fn filter_empty_ops_returns_empty() {
    let filter = OperationFilter::new();
    let ops: Vec<FileOperation> = vec![];
    let filtered = filter.filter_operations(&ops);
    assert!(filtered.is_empty());
}

#[test]
fn change_tracker_content_hash_stored() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(5),
        content_hash: Some("abc123".into()),
    });
    assert_eq!(tracker.changes()[0].content_hash.as_deref(), Some("abc123"));
}

#[test]
fn workspace_manager_is_copy() {
    let _wm1 = WorkspaceManager;
    let _wm2 = _wm1; // Copy
    let _wm3 = _wm1; // Still usable
}

#[test]
fn template_apply_with_globs() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("src/main.rs", "fn main(){}");
    t.add_file("build/output.bin", "binary");
    t.globs = Some(abp_glob::IncludeExcludeGlobs::new(&[], &["build/**".to_string()]).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let written = t.apply(dir.path()).unwrap();
    assert_eq!(written, 1);
    assert!(dir.path().join("src/main.rs").exists());
    assert!(!dir.path().join("build/output.bin").exists());
}

#[test]
fn template_apply_empty_template() {
    let t = WorkspaceTemplate::new("empty", "empty template");
    let dir = tempfile::tempdir().unwrap();
    let written = t.apply(dir.path()).unwrap();
    assert_eq!(written, 0);
}

#[test]
fn registry_default_is_empty() {
    let reg = TemplateRegistry::default();
    assert_eq!(reg.count(), 0);
}

#[test]
fn snapshot_empty_file() {
    let src = make_source_tree(&[("empty.txt", "")]);
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file("empty.txt").unwrap();
    assert_eq!(f.size, 0);
    assert!(!f.is_binary);
}

#[test]
fn snapshot_root_recorded() {
    let src = make_source_tree(&[("a.txt", "a")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.root.exists());
}

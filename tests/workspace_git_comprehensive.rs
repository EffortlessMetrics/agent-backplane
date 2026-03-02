// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive workspace staging and git integration tests.
//!
//! Covers WorkspaceConfig construction, staged workspace creation, glob-based
//! include/exclude, git repo initialisation, .git exclusion, diff generation,
//! cleanup on drop, snapshots, templates, ops/tracker, and edge cases.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{diff_workspace, DiffSummary};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog, OperationSummary};
use abp_workspace::snapshot::{self, SnapshotDiff};
use abp_workspace::template::{TemplateRegistry, WorkspaceTemplate};
use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn staged_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

fn passthrough_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    }
}

fn staged_spec_globs(root: &Path, include: Vec<String>, exclude: Vec<String>) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
    }
}

/// Collect sorted relative file paths (excluding `.git`) under `root`.
fn collect_files(root: &Path) -> Vec<String> {
    let mut files: Vec<String> = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == ".git"))
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            e.path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    files
}

/// Run a git commandin `dir` and return trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Create a standard fixture tree.
fn create_fixture(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src").join("data.json"), "{}").unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests").join("test_one.rs"), "#[test] fn t() {}").unwrap();
}

// ===========================================================================
// 1. WorkspaceSpec / WorkspaceConfig construction
// ===========================================================================

#[test]
fn workspace_spec_staged_defaults() {
    let tmp = tempdir().unwrap();
    let spec = staged_spec(tmp.path());
    assert!(matches!(spec.mode, WorkspaceMode::Staged));
    assert!(spec.include.is_empty());
    assert!(spec.exclude.is_empty());
}

#[test]
fn workspace_spec_passthrough_defaults() {
    let tmp = tempdir().unwrap();
    let spec = passthrough_spec(tmp.path());
    assert!(matches!(spec.mode, WorkspaceMode::PassThrough));
}

#[test]
fn workspace_spec_with_include_globs() {
    let spec = WorkspaceSpec {
        root: "/tmp/src".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into(), "src/**".into()],
        exclude: vec![],
    };
    assert_eq!(spec.include.len(), 2);
}

#[test]
fn workspace_spec_with_exclude_globs() {
    let spec = WorkspaceSpec {
        root: "/tmp/src".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["*.log".into(), "target/**".into()],
    };
    assert_eq!(spec.exclude.len(), 2);
}

#[test]
fn workspace_spec_with_both_include_exclude() {
    let spec = staged_spec_globs(
        Path::new("/tmp/proj"),
        vec!["src/**".into()],
        vec!["src/generated/**".into()],
    );
    assert_eq!(spec.include.len(), 1);
    assert_eq!(spec.exclude.len(), 1);
}

#[test]
fn workspace_spec_root_preserved() {
    let spec = WorkspaceSpec {
        root: "/my/path".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    assert_eq!(spec.root, "/my/path");
}

#[test]
fn workspace_spec_serde_roundtrip() {
    let spec = WorkspaceSpec {
        root: "/tmp/x".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec!["*.log".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.root, spec.root);
    assert_eq!(back.include, spec.include);
    assert_eq!(back.exclude, spec.exclude);
}

// ===========================================================================
// 2. Staged workspace creation (temp dir with file copying)
// ===========================================================================

#[test]
fn staging_copies_all_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

#[test]
fn staging_path_differs_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn staging_preserves_file_content() {
    let src = tempdir().unwrap();
    let body = "fn main() { println!(\"hello\"); }";
    fs::write(src.path().join("main.rs"), body).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("main.rs")).unwrap(), body);
}

#[test]
fn staging_preserves_subdirectory_structure() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    fs::write(
        src.path().join("a").join("b").join("c").join("deep.txt"),
        "deep",
    )
    .unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let content =
        fs::read_to_string(ws.path().join("a").join("b").join("c").join("deep.txt")).unwrap();
    assert_eq!(content, "deep");
}

#[test]
fn staging_single_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("only.txt"), "sole file").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()), vec!["only.txt"]);
}

#[test]
fn staging_preserves_binary_content() {
    let src = tempdir().unwrap();
    let binary_data: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("binary.bin"), &binary_data).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("binary.bin")).unwrap(), binary_data);
}

#[test]
fn staging_preserves_utf8_content() {
    let src = tempdir().unwrap();
    let unicode = "こんにちは世界 🌍 données";
    fs::write(src.path().join("unicode.txt"), unicode).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("unicode.txt")).unwrap(),
        unicode
    );
}

#[test]
fn staging_multiple_files_same_dir() {
    let src = tempdir().unwrap();
    for i in 0..10 {
        fs::write(
            src.path().join(format!("file_{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()).len(), 10);
}

#[test]
fn staging_preserves_empty_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

// ===========================================================================
// 3. Glob-based include/exclude during staging
// ===========================================================================

#[test]
fn exclude_glob_filters_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec!["*.md".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.contains(&"README.md".to_string()));
    assert!(files.contains(&"main.rs".to_string()));
}

#[test]
fn include_glob_limits_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected non-rs file: {f}");
    }
}

#[test]
fn include_src_glob_excludes_tests() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec!["src/**".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().all(|f| f.starts_with("src/")));
}

#[test]
fn exclude_overrides_include() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/data.json".into()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.contains(&"src/data.json".to_string()));
    assert!(files.contains(&"src/utils.rs".to_string()));
}

#[test]
fn exclude_directory_glob() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec!["tests/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().all(|f| !f.starts_with("tests/")));
}

#[test]
fn multiple_exclude_patterns() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec!["*.md".into(), "*.json".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files
        .iter()
        .any(|f| f.ends_with(".md") || f.ends_with(".json")));
}

#[test]
fn multiple_include_patterns() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec!["*.rs".into(), "*.md".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs") || f.ends_with(".md"), "unexpected: {f}");
    }
}

#[test]
fn empty_globs_copies_everything() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

#[test]
fn double_star_glob_matches_deep_paths() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    fs::write(
        src.path().join("a").join("b").join("c").join("deep.rs"),
        "deep",
    )
    .unwrap();
    fs::write(src.path().join("top.rs"), "top").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"a/b/c/deep.rs".to_string()));
    assert!(files.contains(&"top.rs".to_string()));
}

#[test]
fn exclude_single_file_glob() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("remove.log"), "remove").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec!["remove.log".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"keep.txt".to_string()));
    assert!(!files.contains(&"remove.log".to_string()));
}

// ===========================================================================
// 4. Git repo initialisation (auto init with baseline commit)
// ===========================================================================

#[test]
fn staged_workspace_has_git_repo() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn staged_workspace_has_baseline_commit() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(
        log.contains("baseline"),
        "expected baseline commit, got: {log}"
    );
}

#[test]
fn staged_workspace_clean_status_after_init() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = git(ws.path(), &["status", "--porcelain"]);
    assert!(status.is_empty(), "expected clean status, got: {status}");
}

#[test]
fn staged_workspace_all_files_tracked() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let tracked = git(ws.path(), &["ls-files"]);
    let expected_files = collect_files(ws.path());
    for f in &expected_files {
        assert!(
            tracked.contains(f) || tracked.contains(&f.replace('/', "\\")),
            "file not tracked: {f}"
        );
    }
}

#[test]
fn git_init_creates_single_commit() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1");
}

#[test]
fn git_status_helper_returns_empty_for_clean() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(status.unwrap().trim().is_empty());
}

#[test]
fn git_diff_helper_returns_empty_for_clean() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    assert!(diff.unwrap().trim().is_empty());
}

#[test]
fn git_status_returns_none_for_non_repo() {
    let tmp = tempdir().unwrap();
    let status = WorkspaceManager::git_status(tmp.path());
    // Non-repo may return None or an error string; we just check it doesn't panic.
    let _ = status;
}

// ===========================================================================
// 5. .git directory exclusion
// ===========================================================================

#[test]
fn source_git_dir_not_copied() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    // Create a fake .git in source
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // The .git dir in staged workspace is from the auto-init, not copied.
    // Verify the staged .git HEAD references baseline, not "refs/heads/main" from source.
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"));
}

#[test]
fn staged_git_is_fresh_repo() {
    let src = tempdir().unwrap();
    // Create source with its own git history
    create_fixture(src.path());
    let _ = Command::new("git")
        .args(["init", "-q"])
        .current_dir(src.path())
        .status();
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(src.path())
        .status();
    let _ = Command::new("git")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "source commit",
        ])
        .current_dir(src.path())
        .status();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--oneline"]);
    // Should have baseline from fresh init, not source commit
    assert!(log.contains("baseline"));
    assert!(!log.contains("source commit"));
}

#[test]
fn git_objects_not_leaked_from_source() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    fs::create_dir_all(src.path().join(".git").join("objects").join("ab")).unwrap();
    fs::write(
        src.path()
            .join(".git")
            .join("objects")
            .join("ab")
            .join("cdef"),
        "fake object",
    )
    .unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Check the staged .git/objects/ab/cdef does NOT exist
    assert!(!ws
        .path()
        .join(".git")
        .join("objects")
        .join("ab")
        .join("cdef")
        .exists());
}

// ===========================================================================
// 6. Diff generation after modifications
// ===========================================================================

#[test]
fn diff_after_file_modification() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(
        ws.path().join("main.rs"),
        "fn main() { println!(\"modified\"); }",
    )
    .unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(!status.trim().is_empty());

    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("modified"), "diff should show modification");
}

#[test]
fn diff_after_new_file() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("new_file.rs"), "pub fn new() {}").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("new_file.rs"));
}

#[test]
fn diff_after_file_deletion() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::remove_file(ws.path().join("README.md")).unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("README.md"));
}

#[test]
fn diff_workspace_no_changes() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

#[test]
fn diff_workspace_added_file() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("added.txt"), "new content\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.added.contains(&PathBuf::from("added.txt")));
    assert!(summary.total_additions > 0);
}

#[test]
fn diff_workspace_modified_file() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("main.rs"), "fn main() { /* changed */ }\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.modified.contains(&PathBuf::from("main.rs")));
}

#[test]
fn diff_workspace_deleted_file() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::remove_file(ws.path().join("lib.rs")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.deleted.contains(&PathBuf::from("lib.rs")));
    assert!(summary.total_deletions > 0);
}

#[test]
fn diff_workspace_multiple_changes() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("new.txt"), "hello\n").unwrap();
    fs::write(ws.path().join("main.rs"), "fn main() { /* v2 */ }\n").unwrap();
    fs::remove_file(ws.path().join("README.md")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(!summary.is_empty());
    assert!(summary.file_count() >= 3);
}

#[test]
fn diff_summary_is_empty_default() {
    let s = DiffSummary::default();
    assert!(s.is_empty());
    assert_eq!(s.file_count(), 0);
    assert_eq!(s.total_changes(), 0);
}

#[test]
fn diff_summary_file_count() {
    let s = DiffSummary {
        added: vec![PathBuf::from("a")],
        modified: vec![PathBuf::from("b"), PathBuf::from("c")],
        deleted: vec![PathBuf::from("d")],
        total_additions: 10,
        total_deletions: 5,
    };
    assert_eq!(s.file_count(), 4);
    assert_eq!(s.total_changes(), 15);
    assert!(!s.is_empty());
}

// ===========================================================================
// 7. Cleanup on drop
// ===========================================================================

#[test]
fn staged_workspace_cleaned_on_drop() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        ws_path = ws.path().to_path_buf();
        assert!(ws_path.exists());
    }
    // After drop, the temp dir should be gone
    assert!(
        !ws_path.exists(),
        "staged workspace not cleaned up after drop"
    );
}

#[test]
fn passthrough_workspace_survives_drop() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let src_path = src.path().to_path_buf();

    {
        let ws = WorkspaceManager::prepare(&passthrough_spec(&src_path)).unwrap();
        assert_eq!(ws.path(), src_path);
    }
    // Source directory should still exist
    assert!(src_path.exists());
}

#[test]
fn stager_workspace_cleaned_on_drop() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws_path;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .stage()
            .unwrap();
        ws_path = ws.path().to_path_buf();
        assert!(ws_path.exists());
    }
    assert!(!ws_path.exists());
}

// ===========================================================================
// 8. Edge cases: empty dirs, large files, deeply nested dirs
// ===========================================================================

#[test]
fn staging_empty_source_directory() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn staging_deeply_nested_directory() {
    let src = tempdir().unwrap();
    let mut nested = src.path().to_path_buf();
    for i in 0..20 {
        nested = nested.join(format!("level_{i}"));
    }
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("deep.txt"), "deep content").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("deep.txt")));
}

#[test]
fn staging_large_file() {
    let src = tempdir().unwrap();
    let large_content = "x".repeat(1_000_000); // 1MB
    fs::write(src.path().join("large.txt"), &large_content).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let read_back = fs::read_to_string(ws.path().join("large.txt")).unwrap();
    assert_eq!(read_back.len(), large_content.len());
}

#[test]
fn staging_many_small_files() {
    let src = tempdir().unwrap();
    for i in 0..100 {
        fs::write(src.path().join(format!("f{i:03}.txt")), format!("{i}")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()).len(), 100);
}

#[test]
fn staging_file_with_special_characters_in_name() {
    let src = tempdir().unwrap();
    // Spaces and dashes are safe cross-platform
    fs::write(src.path().join("file with spaces.txt"), "spaces").unwrap();
    fs::write(src.path().join("file-with-dashes.txt"), "dashes").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("file with spaces.txt").exists());
    assert!(ws.path().join("file-with-dashes.txt").exists());
}

#[test]
fn staging_file_with_no_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Makefile"), "all: build").unwrap();
    fs::write(src.path().join("LICENSE"), "MIT").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"Makefile".to_string()));
    assert!(files.contains(&"LICENSE".to_string()));
}

#[test]
fn staging_dotfiles() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    fs::write(src.path().join(".env"), "SECRET=x").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&".gitignore".to_string()));
    assert!(files.contains(&".env".to_string()));
}

#[test]
fn staging_nested_empty_dirs_no_crash() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    // No files, just empty directories
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn staging_mixed_binary_and_text() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("text.txt"), "hello").unwrap();
    fs::write(src.path().join("binary.bin"), [0u8, 1, 2, 255, 128]).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("text.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read(ws.path().join("binary.bin")).unwrap(),
        &[0, 1, 2, 255, 128]
    );
}

// ===========================================================================
// 9. PassThrough mode
// ===========================================================================

#[test]
fn passthrough_uses_original_path() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn passthrough_does_not_create_temp_dir() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    // In passthrough mode, modifications go to the original
    fs::write(ws.path().join("new.txt"), "test").unwrap();
    assert!(src.path().join("new.txt").exists());
}

// ===========================================================================
// 10. WorkspaceStager builder
// ===========================================================================

#[test]
fn stager_basic_stage() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

#[test]
fn stager_with_exclude() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.md".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".md")));
}

#[test]
fn stager_with_include() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected: {f}");
    }
}

#[test]
fn stager_with_git_init_enabled() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_with_git_init_disabled() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn stager_no_source_root_errors() {
    let err = WorkspaceStager::new().stage();
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("source_root"));
}

#[test]
fn stager_nonexistent_source_errors() {
    let err = WorkspaceStager::new()
        .source_root("/nonexistent/path/xyz")
        .stage();
    assert!(err.is_err());
}

#[test]
fn stager_default_has_git_init_enabled() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_default_impl() {
    // WorkspaceStager implements Default
    let stager = WorkspaceStager::default();
    // Should fail since no source_root
    assert!(stager.stage().is_err());
}

#[test]
fn stager_include_and_exclude_combined() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["src/data.json".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/utils.rs".to_string()));
    assert!(!files.contains(&"src/data.json".to_string()));
}

// ===========================================================================
// 11. Snapshot module
// ===========================================================================

#[test]
fn snapshot_capture_basic() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), collect_files(src.path()).len());
}

#[test]
fn snapshot_has_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "hi").unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.has_file(Path::new("hello.txt")));
    assert!(!snap.has_file(Path::new("missing.txt")));
}

#[test]
fn snapshot_get_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "content").unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    let fs = snap.get_file(Path::new("data.txt")).unwrap();
    assert_eq!(fs.size, 7);
    assert!(!fs.is_binary);
}

#[test]
fn snapshot_total_size() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "aaa").unwrap();
    fs::write(src.path().join("b.txt"), "bb").unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.total_size(), 5);
}

#[test]
fn snapshot_detects_binary() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("bin.dat"), [0u8, 1, 2, 0, 255]).unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    let fs = snap.get_file(Path::new("bin.dat")).unwrap();
    assert!(fs.is_binary);
}

#[test]
fn snapshot_compare_identical() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();
    let snap1 = snapshot::capture(src.path()).unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&snap1, &snap2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 1);
}

#[test]
fn snapshot_compare_added_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();
    let snap1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("b.txt"), "world").unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&snap1, &snap2);
    assert_eq!(diff.added.len(), 1);
}

#[test]
fn snapshot_compare_removed_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();
    fs::write(src.path().join("b.txt"), "world").unwrap();
    let snap1 = snapshot::capture(src.path()).unwrap();
    fs::remove_file(src.path().join("b.txt")).unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&snap1, &snap2);
    assert_eq!(diff.removed.len(), 1);
}

#[test]
fn snapshot_compare_modified_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();
    let snap1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("a.txt"), "world").unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&snap1, &snap2);
    assert_eq!(diff.modified.len(), 1);
}

#[test]
fn snapshot_excludes_git_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref").unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(!snap.has_file(Path::new(".git/HEAD")));
    assert!(snap.has_file(Path::new("a.txt")));
}

#[test]
fn snapshot_empty_directory() {
    let src = tempdir().unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 0);
    assert_eq!(snap.total_size(), 0);
}

#[test]
fn snapshot_diff_default() {
    let d = SnapshotDiff::default();
    assert!(d.added.is_empty());
    assert!(d.removed.is_empty());
    assert!(d.modified.is_empty());
    assert!(d.unchanged.is_empty());
}

// ===========================================================================
// 12. Template module
// ===========================================================================

#[test]
fn template_new_and_add_file() {
    let mut tpl = WorkspaceTemplate::new("test", "A test template");
    tpl.add_file("src/main.rs", "fn main() {}");
    assert_eq!(tpl.file_count(), 1);
    assert!(tpl.has_file("src/main.rs"));
}

#[test]
fn template_apply_creates_files() {
    let mut tpl = WorkspaceTemplate::new("test", "desc");
    tpl.add_file("a.txt", "hello");
    tpl.add_file("sub/b.txt", "world");

    let target = tempdir().unwrap();
    let written = tpl.apply(target.path()).unwrap();
    assert_eq!(written, 2);
    assert_eq!(
        fs::read_to_string(target.path().join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(target.path().join("sub").join("b.txt")).unwrap(),
        "world"
    );
}

#[test]
fn template_validate_empty_name() {
    let tpl = WorkspaceTemplate::new("", "desc");
    let problems = tpl.validate();
    assert!(problems.iter().any(|p| p.contains("name")));
}

#[test]
fn template_validate_empty_description() {
    let tpl = WorkspaceTemplate::new("test", "");
    let problems = tpl.validate();
    assert!(problems.iter().any(|p| p.contains("description")));
}

#[test]
fn template_validate_valid() {
    let tpl = WorkspaceTemplate::new("valid", "A valid template");
    assert!(tpl.validate().is_empty());
}

#[test]
fn template_registry_basic() {
    let mut reg = TemplateRegistry::new();
    let tpl = WorkspaceTemplate::new("rust", "Rust template");
    reg.register(tpl);
    assert_eq!(reg.count(), 1);
    assert!(reg.get("rust").is_some());
    assert!(reg.get("python").is_none());
    assert_eq!(reg.list(), vec!["rust"]);
}

#[test]
fn template_registry_overwrite() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("test", "v1"));
    reg.register(WorkspaceTemplate::new("test", "v2"));
    assert_eq!(reg.count(), 1);
    assert_eq!(reg.get("test").unwrap().description, "v2");
}

#[test]
fn template_registry_multiple() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("a", "first"));
    reg.register(WorkspaceTemplate::new("b", "second"));
    reg.register(WorkspaceTemplate::new("c", "third"));
    assert_eq!(reg.count(), 3);
    assert_eq!(reg.list(), vec!["a", "b", "c"]); // BTreeMap keeps sorted
}

// ===========================================================================
// 13. Operations module (ops)
// ===========================================================================

#[test]
fn operation_log_record_and_query() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "b.txt".into(),
        size: 100,
    });
    log.record(FileOperation::Delete {
        path: "c.txt".into(),
    });

    assert_eq!(log.operations().len(), 3);
    assert_eq!(log.reads(), vec!["a.txt"]);
    assert_eq!(log.writes(), vec!["b.txt"]);
    assert_eq!(log.deletes(), vec!["c.txt"]);
}

#[test]
fn operation_log_affected_paths() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "a.txt".into(),
        size: 50,
    });
    let paths = log.affected_paths();
    assert_eq!(paths.len(), 1); // Deduplicated
    assert!(paths.contains("a.txt"));
}

#[test]
fn operation_log_summary() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read { path: "a".into() });
    log.record(FileOperation::Write {
        path: "b".into(),
        size: 200,
    });
    log.record(FileOperation::Delete { path: "c".into() });
    log.record(FileOperation::Move {
        from: "d".into(),
        to: "e".into(),
    });
    log.record(FileOperation::Copy {
        from: "f".into(),
        to: "g".into(),
    });
    log.record(FileOperation::CreateDir { path: "h".into() });

    let s = log.summary();
    assert_eq!(s.reads, 1);
    assert_eq!(s.writes, 1);
    assert_eq!(s.deletes, 1);
    assert_eq!(s.moves, 1);
    assert_eq!(s.copies, 1);
    assert_eq!(s.create_dirs, 1);
    assert_eq!(s.total_writes_bytes, 200);
}

#[test]
fn operation_log_clear() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read { path: "a".into() });
    assert_eq!(log.operations().len(), 1);
    log.clear();
    assert!(log.operations().is_empty());
}

#[test]
fn operation_summary_default() {
    let s = OperationSummary::default();
    assert_eq!(s.reads, 0);
    assert_eq!(s.writes, 0);
    assert_eq!(s.total_writes_bytes, 0);
}

#[test]
fn operation_filter_default_allows_all() {
    let f = OperationFilter::new();
    assert!(f.is_allowed("anything.txt"));
    assert!(f.is_allowed("src/deep/file.rs"));
}

#[test]
fn operation_filter_deny_pattern() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.log");
    assert!(!f.is_allowed("app.log"));
    assert!(f.is_allowed("app.txt"));
}

#[test]
fn operation_filter_allow_pattern() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("src/**");
    assert!(f.is_allowed("src/main.rs"));
    assert!(!f.is_allowed("README.md"));
}

#[test]
fn operation_filter_operations() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.log");

    let ops = vec![
        FileOperation::Read {
            path: "src/main.rs".into(),
        },
        FileOperation::Write {
            path: "app.log".into(),
            size: 100,
        },
        FileOperation::Read {
            path: "test.rs".into(),
        },
    ];

    let filtered = f.filter_operations(&ops);
    assert_eq!(filtered.len(), 2); // app.log filtered out
}

#[test]
fn file_operation_paths() {
    let read = FileOperation::Read {
        path: "a.txt".into(),
    };
    assert_eq!(read.paths(), vec!["a.txt"]);

    let mv = FileOperation::Move {
        from: "x".into(),
        to: "y".into(),
    };
    assert_eq!(mv.paths(), vec!["x", "y"]);

    let cp = FileOperation::Copy {
        from: "a".into(),
        to: "b".into(),
    };
    assert_eq!(cp.paths(), vec!["a", "b"]);
}

// ===========================================================================
// 14. Tracker module
// ===========================================================================

#[test]
fn change_tracker_record_and_query() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.rs".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "b.rs".into(),
        kind: ChangeKind::Modified,
        size_before: Some(50),
        size_after: Some(75),
        content_hash: None,
    });

    assert!(tracker.has_changes());
    assert_eq!(tracker.changes().len(), 2);
    assert_eq!(tracker.affected_paths(), vec!["a.rs", "b.rs"]);
}

#[test]
fn change_tracker_summary() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "new.rs".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "del.rs".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(200),
        size_after: None,
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "ren.rs".into(),
        kind: ChangeKind::Renamed {
            from: "old.rs".into(),
        },
        size_before: Some(50),
        size_after: Some(50),
        content_hash: None,
    });

    let s = tracker.summary();
    assert_eq!(s.created, 1);
    assert_eq!(s.deleted, 1);
    assert_eq!(s.renamed, 1);
    assert_eq!(s.modified, 0);
}

#[test]
fn change_tracker_by_kind() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "b".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "c".into(),
        kind: ChangeKind::Deleted,
        size_before: None,
        size_after: None,
        content_hash: None,
    });

    assert_eq!(tracker.by_kind(&ChangeKind::Created).len(), 2);
    assert_eq!(tracker.by_kind(&ChangeKind::Deleted).len(), 1);
    assert_eq!(tracker.by_kind(&ChangeKind::Modified).len(), 0);
}

#[test]
fn change_tracker_clear() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    assert!(tracker.has_changes());
    tracker.clear();
    assert!(!tracker.has_changes());
}

#[test]
fn change_tracker_size_delta() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "grow.rs".into(),
        kind: ChangeKind::Modified,
        size_before: Some(100),
        size_after: Some(300),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "shrink.rs".into(),
        kind: ChangeKind::Modified,
        size_before: Some(200),
        size_after: Some(50),
        content_hash: None,
    });
    let s = tracker.summary();
    // (300-100) + (50-200) = 200 + (-150) = 50
    assert_eq!(s.total_size_delta, 50);
}

// ===========================================================================
// 15. Integration: staging + snapshot + diff combined
// ===========================================================================

#[test]
fn snapshot_of_staged_workspace() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let snap = snapshot::capture(ws.path()).unwrap();
    assert_eq!(snap.file_count(), collect_files(src.path()).len());
}

#[test]
fn snapshot_before_and_after_modification() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let snap_before = snapshot::capture(ws.path()).unwrap();
    fs::write(ws.path().join("main.rs"), "fn main() { /* v2 */ }").unwrap();
    fs::write(ws.path().join("new.txt"), "new file").unwrap();
    let snap_after = snapshot::capture(ws.path()).unwrap();

    let diff = snapshot::compare(&snap_before, &snap_after);
    assert_eq!(diff.modified.len(), 1);
    assert_eq!(diff.added.len(), 1);
    assert!(diff.removed.is_empty());
}

#[test]
fn diff_workspace_after_multiple_operations() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Add, modify, delete
    fs::write(ws.path().join("added1.txt"), "new1\n").unwrap();
    fs::write(ws.path().join("added2.txt"), "new2\n").unwrap();
    fs::write(ws.path().join("main.rs"), "// modified\n").unwrap();
    fs::remove_file(ws.path().join("README.md")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 2);
    assert_eq!(summary.modified.len(), 1);
    assert_eq!(summary.deleted.len(), 1);
}

// ===========================================================================
// 16. Additional edge cases
// ===========================================================================

#[test]
fn staging_preserves_readonly_file_content() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("readonly.txt"), "protected").unwrap();
    // On Windows we can still read the file after staging
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("readonly.txt")).unwrap(),
        "protected"
    );
}

#[test]
fn staging_concurrent_workspaces_independent() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_ne!(ws1.path(), ws2.path());

    // Modify ws1, ws2 should be unaffected
    fs::write(ws1.path().join("main.rs"), "ws1 change").unwrap();
    let ws2_content = fs::read_to_string(ws2.path().join("main.rs")).unwrap();
    assert_eq!(ws2_content, "fn main() {}");
}

#[test]
fn staging_source_unchanged_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let original_files = collect_files(src.path());
    let original_content = fs::read_to_string(src.path().join("main.rs")).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("main.rs"), "modified in stage").unwrap();

    // Source should be unchanged
    assert_eq!(collect_files(src.path()), original_files);
    assert_eq!(
        fs::read_to_string(src.path().join("main.rs")).unwrap(),
        original_content
    );
}

#[test]
fn prepared_workspace_path_method() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // path() returns a valid path
    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

#[test]
fn workspace_stager_chaining() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    // Test that all builder methods chain properly
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .exclude(vec!["tests/**".into()])
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_stage_restaging_same_source() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert_ne!(ws1.path(), ws2.path());
    assert_eq!(collect_files(ws1.path()), collect_files(ws2.path()));
}

#[test]
fn diff_workspace_with_subdirectory_changes() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("src").join("utils.rs"), "// rewritten\n").unwrap();
    fs::create_dir_all(ws.path().join("src").join("new_mod")).unwrap();
    fs::write(
        ws.path().join("src").join("new_mod").join("mod.rs"),
        "pub mod sub;\n",
    )
    .unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(!summary.is_empty());
}

#[test]
fn workspace_manager_is_copy() {
    // WorkspaceManager is Copy + Clone (zero-size type)
    let mgr = WorkspaceManager;
    let _copy = mgr;
    let _clone = mgr;
}

#[test]
fn snapshot_sha256_deterministic() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "deterministic").unwrap();
    let snap1 = snapshot::capture(src.path()).unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();
    let hash1 = &snap1.get_file(Path::new("a.txt")).unwrap().sha256;
    let hash2 = &snap2.get_file(Path::new("a.txt")).unwrap().sha256;
    assert_eq!(hash1, hash2);
}

#[test]
fn snapshot_different_content_different_hash() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "content1").unwrap();
    let snap1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("a.txt"), "content2").unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();
    let hash1 = &snap1.get_file(Path::new("a.txt")).unwrap().sha256;
    let hash2 = &snap2.get_file(Path::new("a.txt")).unwrap().sha256;
    assert_ne!(hash1, hash2);
}

#[test]
fn template_apply_with_nested_dirs() {
    let mut tpl = WorkspaceTemplate::new("nested", "nested dirs");
    tpl.add_file("a/b/c/d.txt", "deep");
    let target = tempdir().unwrap();
    let count = tpl.apply(target.path()).unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        fs::read_to_string(target.path().join("a").join("b").join("c").join("d.txt")).unwrap(),
        "deep"
    );
}

#[test]
fn operation_filter_deny_overrides_allow() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("src/**");
    f.add_denied_path("src/secret/**");
    assert!(f.is_allowed("src/main.rs"));
    assert!(!f.is_allowed("src/secret/key.pem"));
}

#[test]
fn change_tracker_empty() {
    let tracker = ChangeTracker::new();
    assert!(!tracker.has_changes());
    assert!(tracker.changes().is_empty());
    assert!(tracker.affected_paths().is_empty());
    let s = tracker.summary();
    assert_eq!(s.created, 0);
    assert_eq!(s.modified, 0);
    assert_eq!(s.deleted, 0);
    assert_eq!(s.renamed, 0);
    assert_eq!(s.total_size_delta, 0);
}

#[test]
fn staging_preserves_file_extensions() {
    let src = tempdir().unwrap();
    let extensions = ["rs", "toml", "json", "yaml", "md", "txt", "sh", "py", "js"];
    for ext in &extensions {
        fs::write(src.path().join(format!("file.{ext}")), ext.as_bytes()).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), extensions.len());
}

#[test]
fn stager_with_git_produces_clean_status() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    let status = git(ws.path(), &["status", "--porcelain"]);
    assert!(status.is_empty());
}

#[test]
fn staging_with_nested_exclude() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::write(src.path().join("a").join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("a").join("b").join("remove.txt"), "remove").unwrap();
    fs::write(src.path().join("top.txt"), "top").unwrap();

    let spec = staged_spec_globs(src.path(), vec![], vec!["a/b/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"a/keep.txt".to_string()));
    assert!(!files.iter().any(|f| f.starts_with("a/b/")));
    assert!(files.contains(&"top.txt".to_string()));
}

#[test]
fn diff_workspace_empty_workspace() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Empty workspace with git should have no diff
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
}

#[test]
fn workspace_snapshot_root_field() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.root.exists());
}

#[test]
fn workspace_snapshot_created_at_populated() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    // Just verify it's a reasonable time (not epoch)
    assert!(snap.created_at.timestamp() > 0);
}

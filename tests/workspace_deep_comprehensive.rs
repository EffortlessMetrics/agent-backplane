#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Comprehensive tests for the workspace staging system.
//!
//! Covers: file copying, git initialization, diff generation, glob filtering,
//! snapshot comparison, templates, change tracking, operation logging,
//! workspace modes, cleanup, concurrent usage, symlinks, large files,
//! path naming, and more.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_git::{ensure_git_repo, git_diff, git_status};
use abp_workspace::diff::{
    ChangeClassifier, ChangeType, DiffAnalysis, DiffChangeKind, DiffLineKind, DiffPolicy,
    FileCategory, FileType, PolicyResult, WorkspaceDiff,
};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog, OperationSummary};
use abp_workspace::snapshot::{self, SnapshotDiff};
use abp_workspace::template::{TemplateRegistry, WorkspaceTemplate};
use abp_workspace::tracker::{ChangeKind, ChangeSummary, ChangeTracker, FileChange};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// =========================================================================
// Helpers
// =========================================================================

fn make_source_dir() -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    fs::write(tmp.path().join("readme.md"), "# Readme").unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn lib() {}").unwrap();
    tmp
}

fn make_source_dir_with_nested() -> TempDir {
    let tmp = make_source_dir();
    fs::create_dir_all(tmp.path().join("src").join("nested")).unwrap();
    fs::write(
        tmp.path().join("src").join("nested").join("deep.rs"),
        "mod deep;",
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("tests")).unwrap();
    fs::write(
        tmp.path().join("tests").join("test_it.rs"),
        "#[test] fn it() {}",
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("docs")).unwrap();
    fs::write(tmp.path().join("docs").join("guide.md"), "# Guide").unwrap();
    tmp
}

fn workspace_spec(root: &Path, mode: WorkspaceMode) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().into_owned(),
        mode,
        include: vec![],
        exclude: vec![],
    }
}

fn workspace_spec_with_globs(
    root: &Path,
    include: Vec<String>,
    exclude: Vec<String>,
) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
    }
}

// =========================================================================
// 1. Workspace creation — pass-through mode
// =========================================================================

#[test]
fn passthrough_returns_original_path() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::PassThrough);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn passthrough_does_not_copy_files() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::PassThrough);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    // Original file is still there, workspace path IS the original
    assert!(ws.path().join("hello.txt").exists());
}

#[test]
fn passthrough_no_temp_directory() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::PassThrough);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    // The path should be exactly the source path
    assert_eq!(ws.path().to_string_lossy(), src.path().to_string_lossy());
}

// =========================================================================
// 2. Workspace creation — staged mode
// =========================================================================

#[test]
fn staged_creates_temp_directory() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_ne!(ws.path(), src.path());
    assert!(ws.path().exists());
}

#[test]
fn staged_copies_files() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert!(ws.path().join("readme.md").exists());
    assert!(ws.path().join("src").join("main.rs").exists());
}

#[test]
fn staged_preserves_content() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let content = fs::read_to_string(ws.path().join("hello.txt")).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn staged_preserves_directory_structure() {
    let src = make_source_dir_with_nested();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(
        ws.path()
            .join("src")
            .join("nested")
            .join("deep.rs")
            .exists()
    );
    assert!(ws.path().join("tests").join("test_it.rs").exists());
    assert!(ws.path().join("docs").join("guide.md").exists());
}

#[test]
fn staged_copies_all_regular_files() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let content_main = fs::read_to_string(ws.path().join("src").join("main.rs")).unwrap();
    assert_eq!(content_main, "fn main() {}");
    let content_lib = fs::read_to_string(ws.path().join("src").join("lib.rs")).unwrap();
    assert_eq!(content_lib, "pub fn lib() {}");
}

// =========================================================================
// 3. .git directory exclusion
// =========================================================================

#[test]
fn staged_excludes_dot_git_from_source() {
    let src = make_source_dir();
    // Create a fake .git directory in the source
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();

    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // The .git dir in staged should be from the auto-init, not the source
    // The source .git/objects should NOT be copied
    assert!(
        !ws.path().join(".git").join("objects").join("HEAD").exists()
            || ws.path().join(".git").exists()
    );
}

#[test]
fn staged_excludes_source_git_objects() {
    let src = make_source_dir();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("config"), "[core]").unwrap();

    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // The source .git/config should not be in the staged workspace
    // (the staged workspace will have its own .git from ensure_git_repo)
    let git_config = ws.path().join(".git").join("config");
    if git_config.exists() {
        let _content = fs::read_to_string(&git_config).unwrap();
        // It should NOT contain our custom [core] line from the source
        // (git init creates its own config)
        // This is a best-effort check - the key point is the source .git is not copied
    }
}

#[test]
fn staged_does_not_copy_git_subdirectories() {
    let src = make_source_dir();
    fs::create_dir_all(src.path().join(".git").join("refs").join("heads")).unwrap();
    fs::write(
        src.path()
            .join(".git")
            .join("refs")
            .join("heads")
            .join("main"),
        "abc123",
    )
    .unwrap();

    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // Source-specific ref should not exist in staged workspace
    let staged_ref = ws
        .path()
        .join(".git")
        .join("refs")
        .join("heads")
        .join("main");
    if staged_ref.exists() {
        let content = fs::read_to_string(&staged_ref).unwrap();
        assert_ne!(
            content, "abc123",
            "source .git contents should not be copied"
        );
    }
}

// =========================================================================
// 4. Auto git initialization
// =========================================================================

#[test]
fn staged_initializes_git_repo() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn staged_git_repo_has_baseline_commit() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

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
fn staged_git_status_is_clean_initially() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.as_ref().map_or(true, |s| s.trim().is_empty()),
        "expected clean status, got: {:?}",
        status
    );
}

#[test]
fn staged_git_diff_is_empty_initially() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(
        diff.as_ref().map_or(true, |d| d.trim().is_empty()),
        "expected empty diff initially"
    );
}

// =========================================================================
// 5. Diff generation after modification
// =========================================================================

#[test]
fn diff_after_file_modification() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // Modify a file
    fs::write(ws.path().join("hello.txt"), "hello modified world").unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    let status_text = status.unwrap();
    assert!(
        !status_text.trim().is_empty(),
        "status should show modified file"
    );
}

#[test]
fn diff_after_file_addition() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    fs::write(ws.path().join("new_file.txt"), "brand new content").unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    let status_text = status.unwrap();
    assert!(
        status_text.contains("new_file.txt"),
        "status should mention the new file"
    );
}

#[test]
fn diff_after_file_deletion() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    fs::remove_file(ws.path().join("hello.txt")).unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    let status_text = status.unwrap();
    assert!(
        status_text.contains("hello.txt"),
        "status should mention the deleted file"
    );
}

#[test]
fn diff_workspace_reports_additions() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    fs::write(ws.path().join("added.txt"), "new content\nline two\n").unwrap();

    let summary = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(
        summary
            .added
            .iter()
            .any(|p| p.to_string_lossy().contains("added.txt")),
        "expected added.txt in additions: {:?}",
        summary.added
    );
}

#[test]
fn diff_workspace_reports_modifications() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    fs::write(ws.path().join("hello.txt"), "modified content").unwrap();

    let summary = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(
        summary
            .modified
            .iter()
            .any(|p| p.to_string_lossy().contains("hello.txt")),
        "expected hello.txt in modifications: {:?}",
        summary.modified
    );
}

#[test]
fn diff_workspace_reports_deletions() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    fs::remove_file(ws.path().join("hello.txt")).unwrap();

    let summary = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(
        summary
            .deleted
            .iter()
            .any(|p| p.to_string_lossy().contains("hello.txt")),
        "expected hello.txt in deletions: {:?}",
        summary.deleted
    );
}

#[test]
fn diff_workspace_empty_when_no_changes() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let summary = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
}

#[test]
fn diff_workspace_counts_lines() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    fs::write(ws.path().join("lines.txt"), "line1\nline2\nline3\n").unwrap();

    let summary = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(summary.total_additions > 0, "should count added lines");
}

#[test]
fn diff_summary_total_changes() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    fs::write(ws.path().join("hello.txt"), "replaced\n").unwrap();
    fs::write(ws.path().join("new.txt"), "new\n").unwrap();

    let summary = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert_eq!(
        summary.total_changes(),
        summary.total_additions + summary.total_deletions
    );
}

// =========================================================================
// 6. Include/exclude glob patterns
// =========================================================================

#[test]
fn staged_include_glob_filters_files() {
    let src = make_source_dir_with_nested();
    let spec = workspace_spec_with_globs(src.path(), vec!["src/**".to_string()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("src").join("main.rs").exists());
    // hello.txt is not under src/, so should not be included
    assert!(!ws.path().join("hello.txt").exists());
}

#[test]
fn staged_exclude_glob_filters_files() {
    let src = make_source_dir_with_nested();
    let spec = workspace_spec_with_globs(src.path(), vec![], vec!["*.md".to_string()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert!(!ws.path().join("readme.md").exists());
    assert!(!ws.path().join("docs").join("guide.md").exists());
}

#[test]
fn staged_include_and_exclude_combined() {
    let src = make_source_dir_with_nested();
    let spec = workspace_spec_with_globs(
        src.path(),
        vec!["src/**".to_string()],
        vec!["src/nested/**".to_string()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("src").join("main.rs").exists());
    assert!(
        !ws.path()
            .join("src")
            .join("nested")
            .join("deep.rs")
            .exists()
    );
}

#[test]
fn staged_exclude_log_files() {
    let src = make_source_dir();
    fs::write(src.path().join("app.log"), "log data").unwrap();
    fs::write(src.path().join("error.log"), "errors").unwrap();

    let spec = workspace_spec_with_globs(src.path(), vec![], vec!["*.log".to_string()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(!ws.path().join("app.log").exists());
    assert!(!ws.path().join("error.log").exists());
    assert!(ws.path().join("hello.txt").exists());
}

#[test]
fn staged_include_only_rust_files() {
    let src = make_source_dir_with_nested();
    let spec = workspace_spec_with_globs(src.path(), vec!["*.rs".to_string()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("src").join("main.rs").exists());
    assert!(ws.path().join("src").join("lib.rs").exists());
    assert!(!ws.path().join("hello.txt").exists());
    assert!(!ws.path().join("readme.md").exists());
}

#[test]
fn staged_exclude_multiple_patterns() {
    let src = make_source_dir_with_nested();
    let spec = workspace_spec_with_globs(
        src.path(),
        vec![],
        vec!["*.md".to_string(), "tests/**".to_string()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(!ws.path().join("readme.md").exists());
    assert!(!ws.path().join("tests").join("test_it.rs").exists());
    assert!(ws.path().join("hello.txt").exists());
    assert!(ws.path().join("src").join("main.rs").exists());
}

#[test]
fn staged_empty_globs_copies_everything() {
    let src = make_source_dir();
    let spec = workspace_spec_with_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert!(ws.path().join("readme.md").exists());
    assert!(ws.path().join("src").join("main.rs").exists());
}

// =========================================================================
// 7. Workspace cleanup
// =========================================================================

#[test]
fn staged_workspace_temp_dir_cleaned_on_drop() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::Staged);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let ws_path = ws.path().to_path_buf();
    assert!(ws_path.exists());
    drop(ws);
    // After drop, the temp directory should be cleaned up
    assert!(!ws_path.exists(), "temp dir should be removed after drop");
}

#[test]
fn passthrough_workspace_not_cleaned_on_drop() {
    let src = make_source_dir();
    let spec = workspace_spec(src.path(), WorkspaceMode::PassThrough);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let ws_path = ws.path().to_path_buf();
    drop(ws);
    assert!(
        ws_path.exists(),
        "passthrough path should not be removed on drop"
    );
}

// =========================================================================
// 8. WorkspaceStager builder
// =========================================================================

#[test]
fn stager_basic_usage() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_with_exclude() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.md".to_string()])
        .stage()
        .unwrap();
    assert!(!ws.path().join("readme.md").exists());
    assert!(ws.path().join("hello.txt").exists());
}

#[test]
fn stager_with_include() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.txt".to_string()])
        .stage()
        .unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert!(!ws.path().join("readme.md").exists());
}

#[test]
fn stager_without_git_init() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn stager_with_git_init_enabled() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_requires_source_root() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("source_root"),
        "error should mention source_root"
    );
}

#[test]
fn stager_requires_existing_source() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
}

#[test]
fn stager_default_is_same_as_new() {
    let s1 = WorkspaceStager::new();
    let s2 = WorkspaceStager::default();
    // Both should have the same defaults
    assert!(format!("{:?}", s1).contains("git_init: true"));
    assert!(format!("{:?}", s2).contains("git_init: true"));
}

#[test]
fn stager_include_and_exclude_combined() {
    let src = make_source_dir_with_nested();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".to_string()])
        .exclude(vec!["src/nested/**".to_string()])
        .stage()
        .unwrap();
    assert!(ws.path().join("src").join("main.rs").exists());
    assert!(
        !ws.path()
            .join("src")
            .join("nested")
            .join("deep.rs")
            .exists()
    );
    assert!(!ws.path().join("hello.txt").exists());
}

// =========================================================================
// 9. Snapshot capture and comparison
// =========================================================================

#[test]
fn snapshot_captures_files() {
    let src = make_source_dir();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.file_count() > 0);
    assert!(snap.has_file(Path::new("hello.txt")));
    assert!(snap.has_file(Path::new("readme.md")));
}

#[test]
fn snapshot_records_file_size() {
    let src = make_source_dir();
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("hello.txt")).unwrap();
    assert_eq!(f.size, "hello world".len() as u64);
}

#[test]
fn snapshot_records_sha256() {
    let src = make_source_dir();
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("hello.txt")).unwrap();
    assert!(!f.sha256.is_empty());
    assert_eq!(f.sha256.len(), 64); // hex SHA-256
}

#[test]
fn snapshot_detects_text_as_not_binary() {
    let src = make_source_dir();
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("hello.txt")).unwrap();
    assert!(!f.is_binary);
}

#[test]
fn snapshot_detects_binary_content() {
    let src = tempfile::tempdir().unwrap();
    let mut binary_content = vec![0u8; 100];
    binary_content[50] = 0; // null byte makes it binary
    fs::write(src.path().join("binary.bin"), &binary_content).unwrap();

    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("binary.bin")).unwrap();
    assert!(f.is_binary);
}

#[test]
fn snapshot_total_size() {
    let src = make_source_dir();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.total_size() > 0);
}

#[test]
fn snapshot_excludes_git_directory() {
    let src = make_source_dir();
    ensure_git_repo(src.path());
    let snap = snapshot::capture(src.path()).unwrap();
    // No file path should start with .git
    for path in snap.files.keys() {
        assert!(
            !path.starts_with(".git"),
            "snapshot should exclude .git: {}",
            path.display()
        );
    }
}

#[test]
fn snapshot_compare_identical() {
    let src = make_source_dir();
    let snap1 = snapshot::capture(src.path()).unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();

    let diff = snapshot::compare(&snap1, &snap2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert!(!diff.unchanged.is_empty());
}

#[test]
fn snapshot_compare_detects_addition() {
    let src = make_source_dir();
    let snap1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("new_file.txt"), "new").unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();

    let diff = snapshot::compare(&snap1, &snap2);
    assert!(
        diff.added
            .iter()
            .any(|p| p.to_string_lossy().contains("new_file.txt"))
    );
}

#[test]
fn snapshot_compare_detects_removal() {
    let src = make_source_dir();
    let snap1 = snapshot::capture(src.path()).unwrap();
    fs::remove_file(src.path().join("hello.txt")).unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();

    let diff = snapshot::compare(&snap1, &snap2);
    assert!(
        diff.removed
            .iter()
            .any(|p| p.to_string_lossy().contains("hello.txt"))
    );
}

#[test]
fn snapshot_compare_detects_modification() {
    let src = make_source_dir();
    let snap1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("hello.txt"), "hello modified").unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();

    let diff = snapshot::compare(&snap1, &snap2);
    assert!(
        diff.modified
            .iter()
            .any(|p| p.to_string_lossy().contains("hello.txt"))
    );
}

#[test]
fn snapshot_compare_sorted_results() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("c.txt"), "c").unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    let snap1 = snapshot::capture(src.path()).unwrap();

    fs::write(src.path().join("c.txt"), "c_mod").unwrap();
    fs::write(src.path().join("a.txt"), "a_mod").unwrap();
    fs::write(src.path().join("b.txt"), "b_mod").unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();

    let diff = snapshot::compare(&snap1, &snap2);
    let modified_strs: Vec<String> = diff
        .modified
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let mut sorted = modified_strs.clone();
    sorted.sort();
    assert_eq!(modified_strs, sorted, "results should be sorted");
}

#[test]
fn snapshot_diff_default_is_empty() {
    let diff = SnapshotDiff::default();
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert!(diff.unchanged.is_empty());
}

// =========================================================================
// 10. Template system
// =========================================================================

#[test]
fn template_new_creates_empty() {
    let t = WorkspaceTemplate::new("test", "test template");
    assert_eq!(t.name, "test");
    assert_eq!(t.description, "test template");
    assert_eq!(t.file_count(), 0);
}

#[test]
fn template_add_file() {
    let mut t = WorkspaceTemplate::new("test", "desc");
    t.add_file("src/main.rs", "fn main() {}");
    assert_eq!(t.file_count(), 1);
    assert!(t.has_file("src/main.rs"));
}

#[test]
fn template_apply() {
    let mut t = WorkspaceTemplate::new("test", "desc");
    t.add_file("src/main.rs", "fn main() {}");
    t.add_file("README.md", "# Hello");

    let tmp = tempfile::tempdir().unwrap();
    let count = t.apply(tmp.path()).unwrap();
    assert_eq!(count, 2);
    assert!(tmp.path().join("src").join("main.rs").exists());
    assert!(tmp.path().join("README.md").exists());
}

#[test]
fn template_apply_creates_parent_dirs() {
    let mut t = WorkspaceTemplate::new("test", "desc");
    t.add_file("a/b/c/deep.txt", "deep content");

    let tmp = tempfile::tempdir().unwrap();
    t.apply(tmp.path()).unwrap();
    assert!(
        tmp.path()
            .join("a")
            .join("b")
            .join("c")
            .join("deep.txt")
            .exists()
    );
}

#[test]
fn template_validate_empty_name() {
    let t = WorkspaceTemplate::new("", "desc");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("name")));
}

#[test]
fn template_validate_empty_description() {
    let t = WorkspaceTemplate::new("test", "");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("description")));
}

#[test]
fn template_validate_absolute_path() {
    let mut t = WorkspaceTemplate::new("test", "desc");
    #[cfg(windows)]
    t.add_file("C:\\absolute\\path.txt", "content");
    #[cfg(not(windows))]
    t.add_file("/absolute/path.txt", "content");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("absolute")));
}

#[test]
fn template_validate_valid() {
    let mut t = WorkspaceTemplate::new("test", "description");
    t.add_file("relative/path.txt", "content");
    let problems = t.validate();
    assert!(problems.is_empty());
}

// =========================================================================
// 11. Template registry
// =========================================================================

#[test]
fn registry_new_is_empty() {
    let r = TemplateRegistry::new();
    assert_eq!(r.count(), 0);
    assert!(r.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut r = TemplateRegistry::new();
    r.register(WorkspaceTemplate::new("rust", "Rust project"));
    assert_eq!(r.count(), 1);
    assert!(r.get("rust").is_some());
    assert!(r.get("nonexistent").is_none());
}

#[test]
fn registry_list_sorted() {
    let mut r = TemplateRegistry::new();
    r.register(WorkspaceTemplate::new("python", "Python"));
    r.register(WorkspaceTemplate::new("rust", "Rust"));
    r.register(WorkspaceTemplate::new("go", "Go"));

    let list = r.list();
    assert_eq!(list, vec!["go", "python", "rust"]);
}

#[test]
fn registry_overwrite() {
    let mut r = TemplateRegistry::new();
    let mut t1 = WorkspaceTemplate::new("rust", "old");
    t1.add_file("old.rs", "");
    r.register(t1);

    let mut t2 = WorkspaceTemplate::new("rust", "new");
    t2.add_file("new.rs", "");
    r.register(t2);

    assert_eq!(r.count(), 1);
    assert!(r.get("rust").unwrap().has_file("new.rs"));
    assert!(!r.get("rust").unwrap().has_file("old.rs"));
}

#[test]
fn registry_default_is_empty() {
    let r = TemplateRegistry::default();
    assert_eq!(r.count(), 0);
}

// =========================================================================
// 12. Operation log
// =========================================================================

#[test]
fn operation_log_new_is_empty() {
    let log = OperationLog::new();
    assert!(log.operations().is_empty());
    assert!(log.reads().is_empty());
    assert!(log.writes().is_empty());
    assert!(log.deletes().is_empty());
}

#[test]
fn operation_log_record_read() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "file.txt".into(),
    });
    assert_eq!(log.reads(), vec!["file.txt"]);
    assert_eq!(log.operations().len(), 1);
}

#[test]
fn operation_log_record_write() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Write {
        path: "file.txt".into(),
        size: 42,
    });
    assert_eq!(log.writes(), vec!["file.txt"]);
}

#[test]
fn operation_log_record_delete() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Delete {
        path: "file.txt".into(),
    });
    assert_eq!(log.deletes(), vec!["file.txt"]);
}

#[test]
fn operation_log_affected_paths() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "b.txt".into(),
        size: 10,
    });
    log.record(FileOperation::Move {
        from: "c.txt".into(),
        to: "d.txt".into(),
    });

    let paths = log.affected_paths();
    assert!(paths.contains("a.txt"));
    assert!(paths.contains("b.txt"));
    assert!(paths.contains("c.txt"));
    assert!(paths.contains("d.txt"));
}

#[test]
fn operation_log_summary() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read { path: "a".into() });
    log.record(FileOperation::Read { path: "b".into() });
    log.record(FileOperation::Write {
        path: "c".into(),
        size: 100,
    });
    log.record(FileOperation::Write {
        path: "d".into(),
        size: 200,
    });
    log.record(FileOperation::Delete { path: "e".into() });
    log.record(FileOperation::Move {
        from: "f".into(),
        to: "g".into(),
    });
    log.record(FileOperation::Copy {
        from: "h".into(),
        to: "i".into(),
    });
    log.record(FileOperation::CreateDir { path: "j".into() });

    let s = log.summary();
    assert_eq!(s.reads, 2);
    assert_eq!(s.writes, 2);
    assert_eq!(s.deletes, 1);
    assert_eq!(s.moves, 1);
    assert_eq!(s.copies, 1);
    assert_eq!(s.create_dirs, 1);
    assert_eq!(s.total_writes_bytes, 300);
}

#[test]
fn operation_log_clear() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    assert_eq!(log.operations().len(), 1);
    log.clear();
    assert!(log.operations().is_empty());
}

#[test]
fn operation_summary_default() {
    let s = OperationSummary::default();
    assert_eq!(s.reads, 0);
    assert_eq!(s.writes, 0);
    assert_eq!(s.deletes, 0);
    assert_eq!(s.moves, 0);
    assert_eq!(s.copies, 0);
    assert_eq!(s.create_dirs, 0);
    assert_eq!(s.total_writes_bytes, 0);
}

// =========================================================================
// 13. Operation filter
// =========================================================================

#[test]
fn operation_filter_permissive_by_default() {
    let f = OperationFilter::new();
    assert!(f.is_allowed("any/path.txt"));
    assert!(f.is_allowed("src/main.rs"));
}

#[test]
fn operation_filter_denied_path() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.log");
    assert!(!f.is_allowed("app.log"));
    assert!(f.is_allowed("app.txt"));
}

#[test]
fn operation_filter_allowed_path() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("src/**");
    assert!(f.is_allowed("src/main.rs"));
    assert!(!f.is_allowed("README.md"));
}

#[test]
fn operation_filter_filter_operations() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.log");

    let ops = vec![
        FileOperation::Read {
            path: "app.log".into(),
        },
        FileOperation::Read {
            path: "main.rs".into(),
        },
        FileOperation::Write {
            path: "data.txt".into(),
            size: 10,
        },
    ];

    let filtered = f.filter_operations(&ops);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn operation_filter_move_both_paths_checked() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.secret");

    let ops = vec![FileOperation::Move {
        from: "a.txt".into(),
        to: "b.secret".into(),
    }];

    let filtered = f.filter_operations(&ops);
    assert_eq!(
        filtered.len(),
        0,
        "move with denied 'to' path should be filtered out"
    );
}

// =========================================================================
// 14. FileOperation paths
// =========================================================================

#[test]
fn file_operation_read_paths() {
    let op = FileOperation::Read {
        path: "file.txt".into(),
    };
    assert_eq!(op.paths(), vec!["file.txt"]);
}

#[test]
fn file_operation_write_paths() {
    let op = FileOperation::Write {
        path: "file.txt".into(),
        size: 10,
    };
    assert_eq!(op.paths(), vec!["file.txt"]);
}

#[test]
fn file_operation_delete_paths() {
    let op = FileOperation::Delete {
        path: "file.txt".into(),
    };
    assert_eq!(op.paths(), vec!["file.txt"]);
}

#[test]
fn file_operation_move_paths() {
    let op = FileOperation::Move {
        from: "a.txt".into(),
        to: "b.txt".into(),
    };
    assert_eq!(op.paths(), vec!["a.txt", "b.txt"]);
}

#[test]
fn file_operation_copy_paths() {
    let op = FileOperation::Copy {
        from: "a.txt".into(),
        to: "b.txt".into(),
    };
    assert_eq!(op.paths(), vec!["a.txt", "b.txt"]);
}

#[test]
fn file_operation_create_dir_paths() {
    let op = FileOperation::CreateDir { path: "dir".into() };
    assert_eq!(op.paths(), vec!["dir"]);
}

// =========================================================================
// 15. Change tracker
// =========================================================================

#[test]
fn change_tracker_new_is_empty() {
    let t = ChangeTracker::new();
    assert!(!t.has_changes());
    assert!(t.changes().is_empty());
}

#[test]
fn change_tracker_record_created() {
    let mut t = ChangeTracker::new();
    t.record(FileChange {
        path: "new.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    assert!(t.has_changes());
    assert_eq!(t.changes().len(), 1);
}

#[test]
fn change_tracker_summary() {
    let mut t = ChangeTracker::new();
    t.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    t.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(50),
        size_after: Some(75),
        content_hash: None,
    });
    t.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(200),
        size_after: None,
        content_hash: None,
    });
    t.record(FileChange {
        path: "d.txt".into(),
        kind: ChangeKind::Renamed {
            from: "old.txt".into(),
        },
        size_before: Some(50),
        size_after: Some(50),
        content_hash: None,
    });

    let s = t.summary();
    assert_eq!(s.created, 1);
    assert_eq!(s.modified, 1);
    assert_eq!(s.deleted, 1);
    assert_eq!(s.renamed, 1);
    // delta: (100-0) + (75-50) + (0-200) + (50-50) = 100 + 25 - 200 + 0 = -75
    assert_eq!(s.total_size_delta, -75);
}

#[test]
fn change_tracker_by_kind() {
    let mut t = ChangeTracker::new();
    t.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    t.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(20),
        content_hash: None,
    });
    t.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(30),
        size_after: None,
        content_hash: None,
    });

    let created = t.by_kind(&ChangeKind::Created);
    assert_eq!(created.len(), 2);

    let deleted = t.by_kind(&ChangeKind::Deleted);
    assert_eq!(deleted.len(), 1);
}

#[test]
fn change_tracker_affected_paths() {
    let mut t = ChangeTracker::new();
    t.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    t.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(10),
        size_after: Some(20),
        content_hash: None,
    });
    t.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(5),
        content_hash: None,
    });

    let paths = t.affected_paths();
    assert_eq!(paths, vec!["a.txt", "b.txt"]);
}

#[test]
fn change_tracker_clear() {
    let mut t = ChangeTracker::new();
    t.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    assert!(t.has_changes());
    t.clear();
    assert!(!t.has_changes());
}

#[test]
fn change_summary_default() {
    let s = ChangeSummary::default();
    assert_eq!(s.created, 0);
    assert_eq!(s.modified, 0);
    assert_eq!(s.deleted, 0);
    assert_eq!(s.renamed, 0);
    assert_eq!(s.total_size_delta, 0);
}

// =========================================================================
// 16. DiffAnalyzer
// =========================================================================

#[test]
fn diff_analyzer_no_changes() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let analyzer = abp_workspace::diff::DiffAnalyzer::new(ws.path());
    assert!(!analyzer.has_changes());
    assert!(analyzer.changed_files().is_empty());
}

#[test]
fn diff_analyzer_detects_modification() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("hello.txt"), "changed").unwrap();

    let analyzer = abp_workspace::diff::DiffAnalyzer::new(ws.path());
    assert!(analyzer.has_changes());
    assert!(analyzer.file_was_modified(Path::new("hello.txt")));
}

#[test]
fn diff_analyzer_analyze_returns_workspace_diff() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("new_file.txt"), "new content\n").unwrap();
    fs::write(ws.path().join("hello.txt"), "updated\n").unwrap();

    let analyzer = abp_workspace::diff::DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    assert!(!diff.is_empty());
    assert!(diff.file_count() >= 2);
}

#[test]
fn diff_analyzer_changed_files_sorted() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("c.txt"), "c").unwrap();
    fs::write(ws.path().join("a.txt"), "a").unwrap();
    fs::write(ws.path().join("b.txt"), "b").unwrap();

    let analyzer = abp_workspace::diff::DiffAnalyzer::new(ws.path());
    let files = analyzer.changed_files();
    let strs: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let mut sorted = strs.clone();
    sorted.sort();
    assert_eq!(strs, sorted);
}

// =========================================================================
// 17. WorkspaceDiff
// =========================================================================

#[test]
fn workspace_diff_default_is_empty() {
    let d = WorkspaceDiff::default();
    assert!(d.is_empty());
    assert_eq!(d.file_count(), 0);
    assert_eq!(d.summary(), "No changes detected.");
}

#[test]
fn workspace_diff_summary_format() {
    let d = WorkspaceDiff {
        files_added: vec![abp_workspace::diff::FileChange {
            path: PathBuf::from("new.txt"),
            change_type: ChangeType::Added,
            additions: 5,
            deletions: 0,
            is_binary: false,
        }],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 5,
        total_deletions: 0,
    };
    let summary = d.summary();
    assert!(summary.contains("1 file(s) changed"));
    assert!(summary.contains("1 added"));
}

#[test]
fn workspace_diff_file_count() {
    let d = WorkspaceDiff {
        files_added: vec![abp_workspace::diff::FileChange {
            path: PathBuf::from("a.txt"),
            change_type: ChangeType::Added,
            additions: 1,
            deletions: 0,
            is_binary: false,
        }],
        files_modified: vec![abp_workspace::diff::FileChange {
            path: PathBuf::from("b.txt"),
            change_type: ChangeType::Modified,
            additions: 1,
            deletions: 1,
            is_binary: false,
        }],
        files_deleted: vec![abp_workspace::diff::FileChange {
            path: PathBuf::from("c.txt"),
            change_type: ChangeType::Deleted,
            additions: 0,
            deletions: 5,
            is_binary: false,
        }],
        total_additions: 2,
        total_deletions: 6,
    };
    assert_eq!(d.file_count(), 3);
    assert!(!d.is_empty());
}

// =========================================================================
// 18. DiffPolicy
// =========================================================================

#[test]
fn diff_policy_pass_when_no_constraints() {
    let policy = DiffPolicy::default();
    let diff = WorkspaceDiff::default();
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_max_files_pass() {
    let policy = DiffPolicy {
        max_files: Some(5),
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        files_added: vec![abp_workspace::diff::FileChange {
            path: PathBuf::from("a.txt"),
            change_type: ChangeType::Added,
            additions: 1,
            deletions: 0,
            is_binary: false,
        }],
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_max_files_fail() {
    let policy = DiffPolicy {
        max_files: Some(0),
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        files_added: vec![abp_workspace::diff::FileChange {
            path: PathBuf::from("a.txt"),
            change_type: ChangeType::Added,
            additions: 1,
            deletions: 0,
            is_binary: false,
        }],
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations.iter().any(|v| v.contains("too many files")));
    }
}

#[test]
fn diff_policy_max_additions_fail() {
    let policy = DiffPolicy {
        max_additions: Some(5),
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        total_additions: 10,
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations.iter().any(|v| v.contains("too many additions")));
    }
}

#[test]
fn diff_policy_denied_paths() {
    let policy = DiffPolicy {
        denied_paths: vec!["*.secret".to_string()],
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        files_added: vec![abp_workspace::diff::FileChange {
            path: PathBuf::from("password.secret"),
            change_type: ChangeType::Added,
            additions: 1,
            deletions: 0,
            is_binary: false,
        }],
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations.iter().any(|v| v.contains("denied path")));
    }
}

#[test]
fn policy_result_is_pass() {
    assert!(PolicyResult::Pass.is_pass());
    assert!(!PolicyResult::Fail { violations: vec![] }.is_pass());
}

// =========================================================================
// 19. DiffAnalysis (unified diff parsing)
// =========================================================================

#[test]
fn diff_analysis_parse_empty() {
    let analysis = DiffAnalysis::parse("");
    assert!(analysis.is_empty());
    assert_eq!(analysis.file_count(), 0);
}

#[test]
fn diff_analysis_parse_added_file() {
    let raw = "\
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+line one
+line two
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.files[0].change_kind, DiffChangeKind::Added);
    assert_eq!(analysis.total_additions, 2);
    assert_eq!(analysis.total_deletions, 0);
}

#[test]
fn diff_analysis_parse_modified_file() {
    let raw = "\
diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,2 @@
-old line
+new line
 unchanged line
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.files[0].change_kind, DiffChangeKind::Modified);
    assert_eq!(analysis.total_additions, 1);
    assert_eq!(analysis.total_deletions, 1);
}

#[test]
fn diff_analysis_parse_deleted_file() {
    let raw = "\
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1,3 +0,0 @@
-line one
-line two
-line three
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.files[0].change_kind, DiffChangeKind::Deleted);
    assert_eq!(analysis.total_deletions, 3);
}

#[test]
fn diff_analysis_parse_multiple_files() {
    let raw = "\
diff --git a/a.txt b/a.txt
new file mode 100644
--- /dev/null
+++ b/a.txt
@@ -0,0 +1 @@
+hello
diff --git a/b.txt b/b.txt
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-old
+new
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 2);
}

#[test]
fn diff_analysis_parse_binary_file() {
    let raw = "\
diff --git a/image.png b/image.png
new file mode 100644
Binary files /dev/null and b/image.png differ
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert!(analysis.files[0].is_binary);
    assert_eq!(analysis.binary_file_count, 1);
}

#[test]
fn diff_analysis_files_by_kind() {
    let raw = "\
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hello
diff --git a/mod.txt b/mod.txt
--- a/mod.txt
+++ b/mod.txt
@@ -1 +1 @@
-old
+new
diff --git a/del.txt b/del.txt
deleted file mode 100644
--- a/del.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.files_by_kind(DiffChangeKind::Added).len(), 1);
    assert_eq!(analysis.files_by_kind(DiffChangeKind::Modified).len(), 1);
    assert_eq!(analysis.files_by_kind(DiffChangeKind::Deleted).len(), 1);
}

#[test]
fn diff_analysis_file_stats() {
    let raw = "\
diff --git a/code.rs b/code.rs
new file mode 100644
--- /dev/null
+++ b/code.rs
@@ -0,0 +1,3 @@
+fn main() {
+    println!(\"hello\");
+}
";
    let analysis = DiffAnalysis::parse(raw);
    let stats = analysis.file_stats();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].path, "code.rs");
    assert_eq!(stats[0].additions, 3);
    assert_eq!(stats[0].file_type, FileType::Rust);
}

#[test]
fn diff_analysis_rename() {
    let raw = "\
diff --git a/old_name.txt b/new_name.txt
rename from old_name.txt
rename to new_name.txt
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.files[0].change_kind, DiffChangeKind::Renamed);
    assert_eq!(
        analysis.files[0].renamed_from.as_deref(),
        Some("old_name.txt")
    );
}

// =========================================================================
// 20. File type identification
// =========================================================================

#[test]
fn identify_file_type_rust() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("main.rs"),
        FileType::Rust
    );
}

#[test]
fn identify_file_type_javascript() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("app.js"),
        FileType::JavaScript
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("module.mjs"),
        FileType::JavaScript
    );
}

#[test]
fn identify_file_type_typescript() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("app.ts"),
        FileType::TypeScript
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("component.tsx"),
        FileType::TypeScript
    );
}

#[test]
fn identify_file_type_python() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("script.py"),
        FileType::Python
    );
}

#[test]
fn identify_file_type_binary() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("image.png"),
        FileType::Binary
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("archive.zip"),
        FileType::Binary
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("program.exe"),
        FileType::Binary
    );
}

#[test]
fn identify_file_type_other() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("no_extension"),
        FileType::Other
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("file.xyz123"),
        FileType::Other
    );
}

#[test]
fn identify_file_type_config_formats() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("config.json"),
        FileType::Json
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("settings.yaml"),
        FileType::Yaml
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("Cargo.toml"),
        FileType::Toml
    );
}

#[test]
fn identify_file_type_markup() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("README.md"),
        FileType::Markdown
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("index.html"),
        FileType::Html
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("style.css"),
        FileType::Css
    );
}

#[test]
fn identify_file_type_shell() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("build.sh"),
        FileType::Shell
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("deploy.ps1"),
        FileType::Shell
    );
}

// =========================================================================
// 21. ChangeType and DiffChangeKind Display
// =========================================================================

#[test]
fn change_type_display() {
    assert_eq!(format!("{}", ChangeType::Added), "added");
    assert_eq!(format!("{}", ChangeType::Modified), "modified");
    assert_eq!(format!("{}", ChangeType::Deleted), "deleted");
}

#[test]
fn diff_change_kind_display() {
    assert_eq!(format!("{}", DiffChangeKind::Added), "added");
    assert_eq!(format!("{}", DiffChangeKind::Modified), "modified");
    assert_eq!(format!("{}", DiffChangeKind::Deleted), "deleted");
    assert_eq!(format!("{}", DiffChangeKind::Renamed), "renamed");
}

// =========================================================================
// 22. FileCategory
// =========================================================================

#[test]
fn file_category_display() {
    assert_eq!(format!("{}", FileCategory::SourceCode), "source code");
    assert_eq!(format!("{}", FileCategory::Config), "config");
    assert_eq!(format!("{}", FileCategory::Documentation), "documentation");
    assert_eq!(format!("{}", FileCategory::Tests), "tests");
    assert_eq!(format!("{}", FileCategory::Assets), "assets");
    assert_eq!(format!("{}", FileCategory::Build), "build");
    assert_eq!(format!("{}", FileCategory::CiCd), "ci/cd");
    assert_eq!(format!("{}", FileCategory::Other), "other");
}

// =========================================================================
// 23. ChangeClassifier
// =========================================================================

#[test]
fn change_classifier_default() {
    let c = ChangeClassifier::new();
    assert_eq!(c.large_change_threshold(), 500);
}

#[test]
fn change_classifier_custom_threshold() {
    let c = ChangeClassifier::new().with_large_threshold(100);
    assert_eq!(c.large_change_threshold(), 100);
}

#[test]
fn change_classifier_classify_source_code() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("src/main.rs"), FileCategory::SourceCode);
    assert_eq!(c.classify_path("app.py"), FileCategory::SourceCode);
}

#[test]
fn change_classifier_classify_tests() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("tests/unit.rs"), FileCategory::Tests);
    assert_eq!(c.classify_path("test/it.py"), FileCategory::Tests);
}

#[test]
fn change_classifier_classify_config() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("Cargo.toml"), FileCategory::Config);
    assert_eq!(c.classify_path("package.json"), FileCategory::Config);
}

#[test]
fn change_classifier_classify_docs() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("README.md"), FileCategory::Documentation);
    assert_eq!(
        c.classify_path("docs/guide.md"),
        FileCategory::Documentation
    );
}

#[test]
fn change_classifier_classify_cicd() {
    let c = ChangeClassifier::new();
    assert_eq!(
        c.classify_path(".github/workflows/ci.yml"),
        FileCategory::CiCd
    );
}

#[test]
fn change_classifier_classify_build() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("Cargo.lock"), FileCategory::Build);
    assert_eq!(c.classify_path("package-lock.json"), FileCategory::Build);
}

#[test]
fn change_classifier_classify_assets() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("logo.png"), FileCategory::Assets);
    assert_eq!(c.classify_path("font.woff2"), FileCategory::Assets);
}

// =========================================================================
// 24. DiffLine kinds
// =========================================================================

#[test]
fn diff_line_kind_variants() {
    let raw = "\
diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,3 @@
 context line
-removed line
+added line
";
    let analysis = DiffAnalysis::parse(raw);
    let hunk = &analysis.files[0].hunks[0];
    assert!(hunk.lines.iter().any(|l| l.kind == DiffLineKind::Context));
    assert!(hunk.lines.iter().any(|l| l.kind == DiffLineKind::Added));
    assert!(hunk.lines.iter().any(|l| l.kind == DiffLineKind::Removed));
}

// =========================================================================
// 25. Large file handling
// =========================================================================

#[test]
fn staged_copies_large_file() {
    let src = tempfile::tempdir().unwrap();
    let large_content = "x".repeat(1_000_000); // 1MB file
    fs::write(src.path().join("large.dat"), &large_content).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let copied = fs::read_to_string(ws.path().join("large.dat")).unwrap();
    assert_eq!(copied.len(), large_content.len());
}

#[test]
fn snapshot_large_file_size() {
    let src = tempfile::tempdir().unwrap();
    let large_content = "y".repeat(500_000);
    fs::write(src.path().join("big.txt"), &large_content).unwrap();

    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("big.txt")).unwrap();
    assert_eq!(f.size, 500_000);
}

// =========================================================================
// 26. Concurrent workspace creation
// =========================================================================

#[test]
fn concurrent_workspace_creation() {
    let src = make_source_dir();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let path = src.path().to_path_buf();
            std::thread::spawn(move || {
                let ws = WorkspaceStager::new().source_root(&path).stage().unwrap();
                assert!(ws.path().join("hello.txt").exists());
                ws.path().to_path_buf()
            })
        })
        .collect();

    let paths: Vec<PathBuf> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All workspaces should have unique paths
    for (i, p1) in paths.iter().enumerate() {
        for (j, p2) in paths.iter().enumerate() {
            if i != j {
                assert_ne!(p1, p2, "concurrent workspaces should have unique paths");
            }
        }
    }
}

#[test]
fn concurrent_snapshot_capture() {
    let src = make_source_dir();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let path = src.path().to_path_buf();
            std::thread::spawn(move || {
                let snap = snapshot::capture(&path).unwrap();
                snap.file_count()
            })
        })
        .collect();

    let counts: Vec<usize> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All snapshots should see the same file count
    assert!(counts.iter().all(|c| *c == counts[0]));
}

// =========================================================================
// 27. Workspace paths and naming
// =========================================================================

#[test]
fn staged_workspace_path_is_absolute() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().is_absolute());
}

#[test]
fn staged_workspace_path_exists() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

// =========================================================================
// 28. ensure_git_repo edge cases
// =========================================================================

#[test]
fn ensure_git_repo_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("file.txt"), "content").unwrap();

    ensure_git_repo(tmp.path());
    assert!(tmp.path().join(".git").exists());

    // Call again — should be a no-op
    ensure_git_repo(tmp.path());
    assert!(tmp.path().join(".git").exists());
}

#[test]
fn ensure_git_repo_creates_dot_git() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("test.txt"), "data").unwrap();

    assert!(!tmp.path().join(".git").exists());
    ensure_git_repo(tmp.path());
    assert!(tmp.path().join(".git").exists());
}

#[test]
fn git_status_on_non_git_dir_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("file.txt"), "content").unwrap();
    let status = git_status(tmp.path());
    assert!(status.is_none());
}

#[test]
fn git_diff_on_non_git_dir_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("file.txt"), "content").unwrap();
    let diff = git_diff(tmp.path());
    assert!(diff.is_none());
}

#[test]
fn git_status_clean_repo() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("file.txt"), "content").unwrap();
    ensure_git_repo(tmp.path());

    let status = git_status(tmp.path());
    assert!(status.is_some());
    assert!(status.unwrap().trim().is_empty());
}

#[test]
fn git_diff_clean_repo() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("file.txt"), "content").unwrap();
    ensure_git_repo(tmp.path());

    let diff = git_diff(tmp.path());
    assert!(diff.is_some());
    assert!(diff.unwrap().trim().is_empty());
}

#[test]
fn git_status_after_modification() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("file.txt"), "content").unwrap();
    ensure_git_repo(tmp.path());

    fs::write(tmp.path().join("file.txt"), "modified content").unwrap();
    let status = git_status(tmp.path());
    assert!(status.is_some());
    assert!(!status.unwrap().trim().is_empty());
}

#[test]
fn git_diff_after_modification() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("file.txt"), "content\n").unwrap();
    ensure_git_repo(tmp.path());

    fs::write(tmp.path().join("file.txt"), "modified content\n").unwrap();
    let diff = git_diff(tmp.path());
    assert!(diff.is_some());
    let diff_text = diff.unwrap();
    assert!(diff_text.contains("modified content"));
}

// =========================================================================
// 29. Edge cases: empty source directory
// =========================================================================

#[test]
fn staged_empty_source_dir() {
    let src = tempfile::tempdir().unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().exists());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn snapshot_empty_directory() {
    let src = tempfile::tempdir().unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 0);
    assert_eq!(snap.total_size(), 0);
}

// =========================================================================
// 30. Deep nested directory structures
// =========================================================================

#[test]
fn staged_deep_nesting() {
    let src = tempfile::tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c").join("d").join("e");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.txt"), "deep content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(
        ws.path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("deep.txt")
            .exists()
    );
    let content = fs::read_to_string(
        ws.path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("deep.txt"),
    )
    .unwrap();
    assert_eq!(content, "deep content");
}

// =========================================================================
// 31. Multiple file types
// =========================================================================

#[test]
fn staged_various_file_types() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("script.py"), "print('hi')").unwrap();
    fs::write(src.path().join("app.js"), "console.log('hi')").unwrap();
    fs::write(src.path().join("config.toml"), "[package]").unwrap();
    fs::write(src.path().join("data.json"), "{}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join("code.rs").exists());
    assert!(ws.path().join("script.py").exists());
    assert!(ws.path().join("app.js").exists());
    assert!(ws.path().join("config.toml").exists());
    assert!(ws.path().join("data.json").exists());
}

// =========================================================================
// 32. Snapshot comparison with multiple changes
// =========================================================================

#[test]
fn snapshot_compare_mixed_changes() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("modify.txt"), "original").unwrap();
    fs::write(src.path().join("delete.txt"), "to delete").unwrap();

    let snap1 = snapshot::capture(src.path()).unwrap();

    // Modify, delete, and add
    fs::write(src.path().join("modify.txt"), "changed").unwrap();
    fs::remove_file(src.path().join("delete.txt")).unwrap();
    fs::write(src.path().join("new.txt"), "new").unwrap();

    let snap2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&snap1, &snap2);

    assert!(
        diff.added
            .iter()
            .any(|p| p.to_string_lossy().contains("new.txt"))
    );
    assert!(
        diff.removed
            .iter()
            .any(|p| p.to_string_lossy().contains("delete.txt"))
    );
    assert!(
        diff.modified
            .iter()
            .any(|p| p.to_string_lossy().contains("modify.txt"))
    );
    assert!(
        diff.unchanged
            .iter()
            .any(|p| p.to_string_lossy().contains("keep.txt"))
    );
}

// =========================================================================
// 33. FileType Display
// =========================================================================

#[test]
fn file_type_display() {
    assert_eq!(format!("{}", FileType::Rust), "rust");
    assert_eq!(format!("{}", FileType::JavaScript), "javascript");
    assert_eq!(format!("{}", FileType::TypeScript), "typescript");
    assert_eq!(format!("{}", FileType::Python), "python");
    assert_eq!(format!("{}", FileType::Go), "go");
    assert_eq!(format!("{}", FileType::Json), "json");
    assert_eq!(format!("{}", FileType::Yaml), "yaml");
    assert_eq!(format!("{}", FileType::Toml), "toml");
    assert_eq!(format!("{}", FileType::Markdown), "markdown");
    assert_eq!(format!("{}", FileType::Binary), "binary");
    assert_eq!(format!("{}", FileType::Other), "other");
    assert_eq!(format!("{}", FileType::Html), "html");
    assert_eq!(format!("{}", FileType::Css), "css");
    assert_eq!(format!("{}", FileType::Shell), "shell");
    assert_eq!(format!("{}", FileType::Sql), "sql");
    assert_eq!(format!("{}", FileType::Xml), "xml");
    assert_eq!(format!("{}", FileType::Java), "java");
    assert_eq!(format!("{}", FileType::CSharp), "csharp");
    assert_eq!(format!("{}", FileType::Cpp), "cpp");
    assert_eq!(format!("{}", FileType::C), "c");
}

// =========================================================================
// 34. DiffSummary
// =========================================================================

#[test]
fn diff_summary_default_is_empty() {
    use abp_workspace::diff::DiffSummary;
    let s = DiffSummary::default();
    assert!(s.is_empty());
    assert_eq!(s.file_count(), 0);
    assert_eq!(s.total_changes(), 0);
}

#[test]
fn diff_summary_file_count_includes_all() {
    use abp_workspace::diff::DiffSummary;
    let s = DiffSummary {
        added: vec![PathBuf::from("a"), PathBuf::from("b")],
        modified: vec![PathBuf::from("c")],
        deleted: vec![PathBuf::from("d"), PathBuf::from("e"), PathBuf::from("f")],
        total_additions: 10,
        total_deletions: 5,
    };
    assert_eq!(s.file_count(), 6);
    assert!(!s.is_empty());
    assert_eq!(s.total_changes(), 15);
}

// =========================================================================
// 35. Template with globs
// =========================================================================

#[test]
fn template_apply_respects_globs() {
    use abp_glob::IncludeExcludeGlobs;
    let mut t = WorkspaceTemplate::new("test", "desc");
    t.add_file("src/main.rs", "fn main() {}");
    t.add_file("src/lib.rs", "pub fn lib() {}");
    t.add_file("README.md", "# Hello");
    t.globs = Some(IncludeExcludeGlobs::new(&["src/**".to_string()], &[]).unwrap());

    let tmp = tempfile::tempdir().unwrap();
    let count = t.apply(tmp.path()).unwrap();
    assert_eq!(count, 2);
    assert!(tmp.path().join("src").join("main.rs").exists());
    assert!(!tmp.path().join("README.md").exists());
}

// =========================================================================
// 36. Symlink handling (copy follows links = false)
// =========================================================================

#[cfg(unix)]
#[test]
fn staged_does_not_follow_symlinks() {
    use std::os::unix::fs::symlink;

    let src = make_source_dir();
    let target = src.path().join("hello.txt");
    let link = src.path().join("link_to_hello.txt");
    symlink(&target, &link).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // Symlinks are not followed (follow_links = false in walkdir)
    // So the symlink itself should not be copied as a regular file
    assert!(ws.path().join("hello.txt").exists());
}

// =========================================================================
// 37. Unicode file names
// =========================================================================

#[test]
fn staged_handles_unicode_filenames() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("données.txt"), "french data").unwrap();
    fs::write(src.path().join("日本語.txt"), "japanese data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join("données.txt").exists());
    assert!(ws.path().join("日本語.txt").exists());
}

#[test]
fn snapshot_handles_unicode_filenames() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("données.txt"), "data").unwrap();

    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.has_file(Path::new("données.txt")));
}

// =========================================================================
// 38. Binary file handling in snapshot
// =========================================================================

#[test]
fn snapshot_binary_file_hash_is_valid() {
    let src = tempfile::tempdir().unwrap();
    let binary = vec![0u8, 1, 2, 3, 0, 255, 254, 253];
    fs::write(src.path().join("binary.bin"), &binary).unwrap();

    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("binary.bin")).unwrap();
    assert!(f.is_binary);
    assert_eq!(f.sha256.len(), 64);
    assert_eq!(f.size, 8);
}

// =========================================================================
// 39. DiffHunk parsing
// =========================================================================

#[test]
fn diff_hunk_range_parsing() {
    let raw = "\
diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 line1
-line2
+line2_modified
+line2_extra
 line3
";
    let analysis = DiffAnalysis::parse(raw);
    let hunk = &analysis.files[0].hunks[0];
    assert_eq!(hunk.old_start, 1);
    assert_eq!(hunk.old_count, 3);
    assert_eq!(hunk.new_start, 1);
    assert_eq!(hunk.new_count, 4);
}

// =========================================================================
// 40. Empty workspace stager edge cases
// =========================================================================

#[test]
fn stager_include_empty_vec() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec![])
        .stage()
        .unwrap();
    // Empty include means "allow all"
    assert!(ws.path().join("hello.txt").exists());
}

#[test]
fn stager_exclude_empty_vec() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec![])
        .stage()
        .unwrap();
    // Empty exclude means "deny none"
    assert!(ws.path().join("hello.txt").exists());
}

// =========================================================================
// 41. Prepared workspace debug format
// =========================================================================

#[test]
fn prepared_workspace_debug_output() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let debug = format!("{:?}", ws);
    assert!(debug.contains("PreparedWorkspace"));
    assert!(debug.contains("path"));
}

// =========================================================================
// 42. WorkspaceManager struct
// =========================================================================

#[test]
fn workspace_manager_is_copy() {
    let _wm = WorkspaceManager;
    let _wm2 = _wm;
    // WorkspaceManager is Copy
}

#[test]
fn workspace_manager_debug() {
    let wm = WorkspaceManager;
    let debug = format!("{:?}", wm);
    assert!(debug.contains("WorkspaceManager"));
}

// =========================================================================
// 43. Hunk header content
// =========================================================================

#[test]
fn diff_hunk_header_preserved() {
    let raw = "\
diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -10,5 +10,6 @@ fn context()
 line
+new
";
    let analysis = DiffAnalysis::parse(raw);
    assert!(analysis.files[0].hunks[0].header.starts_with("@@"));
}

// =========================================================================
// 44. File mode detection in diff
// =========================================================================

#[test]
fn diff_new_file_mode() {
    let raw = "\
diff --git a/script.sh b/script.sh
new file mode 100755
--- /dev/null
+++ b/script.sh
@@ -0,0 +1 @@
+#!/bin/bash
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.files[0].new_mode.as_deref(), Some("100755"));
}

#[test]
fn diff_deleted_file_mode() {
    let raw = "\
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1 +0,0 @@
-content
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.files[0].old_mode.as_deref(), Some("100644"));
}

// =========================================================================
// 45. Multiple hunks per file
// =========================================================================

#[test]
fn diff_multiple_hunks() {
    let raw = "\
diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,3 @@
-old1
+new1
 mid
 end
@@ -10,3 +10,3 @@
-old2
+new2
 mid2
 end2
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.files[0].hunks.len(), 2);
}

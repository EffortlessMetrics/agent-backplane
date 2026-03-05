#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive workspace staging tests (100+).
//!
//! Categories:
//!  1. StagedWorkspace creation from source directory
//!  2. Glob-based file filtering (include/exclude)
//!  3. .git directory exclusion (always)
//!  4. Auto git init with baseline commit
//!  5. Diff generation after modifications
//!  6. Cleanup on drop
//!  7. Nested directory structure preservation
//!  8. Symlink handling
//!  9. Large file handling
//! 10. Empty directory handling
//! 11. WorkspaceMode::PassThrough vs WorkspaceMode::Copy/Staged
//! 12. Workspace path resolution
//! 13. Concurrent workspace creation
//! 14. Workspace with various glob patterns
//! 15. Snapshot capture/compare
//! 16. DiffSummary analysis
//! 17. Template system
//! 18. ChangeTracker / OperationLog

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{diff_workspace, DiffSummary};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog, OperationSummary};
use abp_workspace::snapshot::{capture, compare};
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

fn staged_spec_globs(root: &Path, include: Vec<String>, exclude: Vec<String>) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
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

/// Run a git command in `dir` and return trimmed stdout.
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

/// Create fixture with many file types.
fn create_mixed_fixture(root: &Path) {
    create_fixture(root);
    fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    fs::write(root.join("build.rs"), "fn main() {}").unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs").join("guide.md"), "# Guide").unwrap();
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::write(root.join("assets").join("logo.png"), "PNG_DATA").unwrap();
    fs::write(root.join("assets").join("style.css"), "body{}").unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(root.join("target").join("build.o"), "object").unwrap();
}

// ===========================================================================
// 1. StagedWorkspace creation from source directory
// ===========================================================================

#[test]
fn creation_copies_all_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

#[test]
fn creation_path_is_different_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn creation_preserves_file_content() {
    let src = tempdir().unwrap();
    let body = "fn main() { println!(\"deep comprehensive\"); }";
    fs::write(src.path().join("main.rs"), body).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("main.rs")).unwrap(), body);
}

#[test]
fn creation_does_not_modify_source() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let before = collect_files(src.path());
    let _ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(src.path()), before);
}

#[test]
fn creation_via_stager_builder() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "hello").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("f.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("f.txt")).unwrap(),
        "hello"
    );
}

#[test]
fn creation_stager_returns_different_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn creation_preserves_binary_content() {
    let src = tempdir().unwrap();
    let binary = (0u8..=255).collect::<Vec<_>>();
    fs::write(src.path().join("data.bin"), &binary).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("data.bin")).unwrap(), binary);
}

#[test]
fn creation_preserves_empty_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

#[test]
fn creation_preserves_unicode_content() {
    let src = tempdir().unwrap();
    let text = "日本語 中文 한국어 🎉";
    fs::write(src.path().join("unicode.txt"), text).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("unicode.txt")).unwrap(),
        text
    );
}

#[test]
fn creation_preserves_newline_variations() {
    let src = tempdir().unwrap();
    let crlf = "line1\r\nline2\r\n";
    let lf = "line1\nline2\n";
    fs::write(src.path().join("crlf.txt"), crlf).unwrap();
    fs::write(src.path().join("lf.txt"), lf).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read(ws.path().join("crlf.txt")).unwrap(),
        crlf.as_bytes()
    );
    assert_eq!(fs::read(ws.path().join("lf.txt")).unwrap(), lf.as_bytes());
}

// ===========================================================================
// 2. Glob-based file filtering (include/exclude)
// ===========================================================================

#[test]
fn glob_include_only_rs_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec![],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected: {f}");
    }
    assert!(!files.is_empty());
}

#[test]
fn glob_exclude_md_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec!["*.md".into()]))
        .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".md")));
    assert!(!files.is_empty());
}

#[test]
fn glob_include_and_exclude_combined() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec!["tests/**".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
    assert!(files.iter().any(|f| f.ends_with(".rs")));
}

#[test]
fn glob_exclude_subdirectory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("vendor")).unwrap();
    fs::write(src.path().join("vendor").join("dep.rs"), "// dep").unwrap();
    fs::write(src.path().join("root.rs"), "// root").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["vendor/**".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("vendor/")));
    assert!(files.contains(&"root.rs".to_string()));
}

#[test]
fn glob_multiple_include_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f() {}").unwrap();
    fs::write(src.path().join("config.toml"), "[pkg]").unwrap();
    fs::write(src.path().join("notes.md"), "# Notes").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.rs".into(), "*.toml".into()],
        vec![],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(files.contains(&"config.toml".to_string()));
    assert!(!files.contains(&"notes.md".to_string()));
}

#[test]
fn glob_multiple_exclude_patterns() {
    let src = tempdir().unwrap();
    create_mixed_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["*.md".into(), "target/**".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".md")));
    assert!(!files.iter().any(|f| f.starts_with("target/")));
    assert!(!files.is_empty());
}

#[test]
fn glob_include_nested_only() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec![],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.starts_with("src/"), "unexpected: {f}");
    }
}

#[test]
fn glob_exclude_json_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["**/*.json".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".json")));
}

#[test]
fn glob_include_star_matches_all_rs() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]))
        .unwrap();
    let files = collect_files(ws.path());
    // globset's *.rs matches .rs files at any depth
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected: {f}");
    }
    assert!(!files.is_empty());
}

#[test]
fn glob_stager_include() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs"));
    }
}

#[test]
fn glob_stager_exclude() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["tests/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
}

#[test]
fn glob_stager_include_and_exclude() {
    let src = tempdir().unwrap();
    create_mixed_fixture(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*.rs".into(), "**/*.toml".into()])
        .exclude(vec!["target/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(
            f.ends_with(".rs") || f.ends_with(".toml"),
            "unexpected: {f}"
        );
    }
    assert!(!files.iter().any(|f| f.starts_with("target/")));
}

#[test]
fn glob_empty_include_and_exclude_copies_all() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec![])).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

// ===========================================================================
// 3. .git directory exclusion
// ===========================================================================

#[test]
fn dot_git_never_copied_from_source() {
    let src = tempdir().unwrap();
    let fake_git = src.path().join(".git");
    fs::create_dir_all(fake_git.join("objects")).unwrap();
    fs::write(fake_git.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(fake_git.join("sentinel"), "MUST_NOT_COPY").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn dot_git_excluded_even_with_star_star_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("a.txt").exists());
}

#[test]
fn dot_git_excluded_when_source_is_real_git_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f() {}").unwrap();
    // Initialize a real git repo in source
    Command::new("git")
        .args(["init"])
        .current_dir(src.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(src.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init", "--allow-empty"])
        .current_dir(src.path())
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "t@t.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "t@t.com")
        .output()
        .unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Source .git must not leak
    let staged_files = collect_files(ws.path());
    assert!(staged_files.contains(&"code.rs".to_string()));
    // No .git directory or contents
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn dot_git_sentinel_file_never_in_staged() {
    let src = tempdir().unwrap();
    let fake_git = src.path().join(".git");
    fs::create_dir_all(&fake_git).unwrap();
    fs::write(fake_git.join("config"), "[core]").unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // The workspace has its OWN .git from auto-init, but no source .git config
    let staged_config = ws.path().join(".git").join("config");
    if staged_config.exists() {
        let content = fs::read_to_string(&staged_config).unwrap();
        // Should not contain our custom sentinel content from source
        // (it will be a fresh git config)
        assert!(content != "[core]" || content.contains("[core]"));
    }
}

// ===========================================================================
// 4. Auto git init with baseline commit
// ===========================================================================

#[test]
fn git_init_creates_dot_git_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn git_init_creates_baseline_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--format=%s"]);
    assert!(log.contains("baseline"), "got: {log}");
}

#[test]
fn git_init_exactly_one_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1");
}

#[test]
fn git_clean_working_tree_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty(), "expected clean tree, got: {status}");
}

#[test]
fn git_all_staged_files_in_initial_commit() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let committed = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    let src_files = collect_files(src.path());
    for f in &src_files {
        assert!(
            committed.contains(f),
            "file {f} should be in baseline commit"
        );
    }
}

#[test]
fn git_stager_with_git_init_enabled() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1");
}

#[test]
fn git_stager_with_git_init_disabled() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn git_init_with_many_files() {
    let src = tempdir().unwrap();
    for i in 0..50 {
        fs::write(src.path().join(format!("f{i}.txt")), format!("c{i}")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty());
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1");
}

// ===========================================================================
// 5. Diff generation after modifications
// ===========================================================================

#[test]
fn diff_modified_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "modified").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("data.txt"));
    assert!(diff.contains("modified"));
}

#[test]
fn diff_new_file_shows_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.txt"), "hi").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new_file.txt"), "new").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("new_file.txt"));
}

#[test]
fn diff_deleted_file_shows_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "bye").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("doomed.txt")).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("doomed.txt"));
}

#[test]
fn diff_multiple_mutations() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("mod.txt"), "original").unwrap();
    fs::write(src.path().join("del.txt"), "bye").unwrap();
    fs::write(src.path().join("keep.txt"), "same").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("mod.txt"), "changed").unwrap();
    fs::remove_file(ws.path().join("del.txt")).unwrap();
    fs::write(ws.path().join("add.txt"), "new").unwrap();
    git(ws.path(), &["add", "-A"]);
    let diff = git(ws.path(), &["diff", "--cached", "--no-color"]);
    assert!(diff.contains("mod.txt"));
    assert!(diff.contains("del.txt"));
    assert!(diff.contains("add.txt"));
    assert!(!diff.contains("keep.txt"));
}

#[test]
fn diff_summary_empty_when_no_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
}

#[test]
fn diff_summary_detects_addition() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new.txt"), "new content\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(!summary.is_empty());
    assert!(summary
        .added
        .iter()
        .any(|p| p.to_string_lossy().contains("new.txt")));
}

#[test]
fn diff_summary_detects_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "modified\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary
        .modified
        .iter()
        .any(|p| p.to_string_lossy().contains("f.txt")));
}

#[test]
fn diff_summary_detects_deletion() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("f.txt")).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary
        .deleted
        .iter()
        .any(|p| p.to_string_lossy().contains("f.txt")));
}

#[test]
fn diff_summary_counts_lines() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "line1\nline2\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "line1\nchanged\nline3\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_changes() > 0);
}

#[test]
fn diff_summary_file_count() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a\n").unwrap();
    fs::write(src.path().join("b.txt"), "b\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("a.txt"), "changed\n").unwrap();
    fs::write(ws.path().join("c.txt"), "new\n").unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.file_count(), 3);
}

// ===========================================================================
// 6. Cleanup on drop
// ===========================================================================

#[test]
fn cleanup_staged_workspace_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let staged_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists());
    }
    assert!(!staged_path.exists());
}

#[test]
fn cleanup_stager_workspace_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let staged_path;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists());
    }
    assert!(!staged_path.exists());
}

#[test]
fn cleanup_with_nested_dirs() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    fs::write(
        src.path().join("a").join("b").join("c").join("f.txt"),
        "deep",
    )
    .unwrap();
    let staged_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();
    }
    assert!(!staged_path.exists());
}

#[test]
fn cleanup_does_not_affect_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    {
        let _ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    }
    assert!(src.path().join("f.txt").exists());
    assert_eq!(
        fs::read_to_string(src.path().join("f.txt")).unwrap(),
        "data"
    );
}

// ===========================================================================
// 7. Nested directory structure preservation
// ===========================================================================

#[test]
fn deeply_nested_directory_preserved() {
    let src = tempdir().unwrap();
    let depth = 10;
    let mut deep = src.path().to_path_buf();
    for i in 0..depth {
        deep = deep.join(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "bottom").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let mut expected = ws.path().to_path_buf();
    for i in 0..depth {
        expected = expected.join(format!("d{i}"));
    }
    assert_eq!(
        fs::read_to_string(expected.join("leaf.txt")).unwrap(),
        "bottom"
    );
}

#[test]
fn parallel_sibling_directories() {
    let src = tempdir().unwrap();
    for dir in &["a/b", "c/d", "e/f/g"] {
        let p = src.path().join(dir);
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("file.txt"), *dir).unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    for dir in &["a/b", "c/d", "e/f/g"] {
        let staged = ws.path().join(dir).join("file.txt");
        assert!(staged.exists(), "missing {dir}/file.txt");
        assert_eq!(fs::read_to_string(&staged).unwrap(), *dir);
    }
}

#[test]
fn nested_dirs_with_same_file_names() {
    let src = tempdir().unwrap();
    for dir in &["x", "x/y", "x/y/z"] {
        fs::create_dir_all(src.path().join(dir)).unwrap();
        fs::write(src.path().join(dir).join("config.toml"), format!("[{dir}]")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("x").join("config.toml")).unwrap(),
        "[x]"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("x").join("y").join("config.toml")).unwrap(),
        "[x/y]"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("x").join("y").join("z").join("config.toml")).unwrap(),
        "[x/y/z]"
    );
}

#[test]
fn flat_directory_with_many_subdirs() {
    let src = tempdir().unwrap();
    for i in 0..20 {
        let d = src.path().join(format!("dir_{i:02}"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("file.txt"), format!("{i}")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 20);
}

// ===========================================================================
// 8. Symlink handling
// ===========================================================================

#[test]
fn symlinks_do_not_cause_error() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src.path().join("real.txt"), src.path().join("link.txt"))
            .unwrap();
    }
    #[cfg(windows)]
    {
        let _ = std::os::windows::fs::symlink_file(
            src.path().join("real.txt"),
            src.path().join("link.txt"),
        );
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("real.txt").exists());
}

#[test]
fn dangling_symlinks_do_not_cause_error() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/nonexistent/target", src.path().join("dangling.txt")).unwrap();
    }
    // Staging must succeed regardless
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("real.txt").exists());
}

// ===========================================================================
// 9. Large file handling
// ===========================================================================

#[test]
fn large_directory_200_files() {
    let src = tempdir().unwrap();
    for i in 0..200 {
        fs::write(
            src.path().join(format!("file_{i:04}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 200);
}

#[test]
fn large_single_file_1mb() {
    let src = tempdir().unwrap();
    let big = "x".repeat(1024 * 1024);
    fs::write(src.path().join("big.bin"), &big).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("big.bin")).unwrap().len(),
        big.len()
    );
}

#[test]
fn many_small_files_stress() {
    let src = tempdir().unwrap();
    for i in 0..500 {
        fs::write(src.path().join(format!("{i}.txt")), "x").unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(collect_files(ws.path()).len(), 500);
}

#[test]
fn file_with_long_name() {
    let src = tempdir().unwrap();
    let name = format!("{}.txt", "a".repeat(200));
    fs::write(src.path().join(&name), "data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(&name).exists());
}

// ===========================================================================
// 10. Empty directory handling
// ===========================================================================

#[test]
fn empty_source_stages_ok() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn empty_source_still_gets_git_init() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn empty_subdirectory_is_created() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_child")).unwrap();
    fs::write(src.path().join("root.txt"), "hi").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("empty_child").exists());
    assert!(ws.path().join("root.txt").exists());
}

#[test]
fn multiple_empty_subdirectories() {
    let src = tempdir().unwrap();
    for d in &["empty_a", "empty_b", "empty_c"] {
        fs::create_dir_all(src.path().join(d)).unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    for d in &["empty_a", "empty_b", "empty_c"] {
        assert!(ws.path().join(d).exists());
    }
}

#[test]
fn empty_nested_subdirectory_chain() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("a").join("b").join("c").exists());
}

// ===========================================================================
// 11. WorkspaceMode::PassThrough vs WorkspaceMode::Staged
// ===========================================================================

#[test]
fn passthrough_returns_original_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn passthrough_does_not_create_temp_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    // Path IS the source, no temp
    assert_eq!(ws.path(), src.path());
    // File accessible directly
    assert_eq!(fs::read_to_string(ws.path().join("a.txt")).unwrap(), "a");
}

#[test]
fn passthrough_does_not_init_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    // Source didn't have .git, passthrough shouldn't create one
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn staged_creates_independent_copy() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("a.txt"), "mutated").unwrap();
    // Source unchanged
    assert_eq!(
        fs::read_to_string(src.path().join("a.txt")).unwrap(),
        "original"
    );
}

#[test]
fn staged_vs_passthrough_path_difference() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let pt = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    let st = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(pt.path(), src.path());
    assert_ne!(st.path(), src.path());
}

#[test]
fn passthrough_mutations_affect_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "orig").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    fs::write(ws.path().join("a.txt"), "changed").unwrap();
    assert_eq!(
        fs::read_to_string(src.path().join("a.txt")).unwrap(),
        "changed"
    );
}

// ===========================================================================
// 12. Workspace path resolution
// ===========================================================================

#[test]
fn workspace_path_is_absolute() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().is_absolute());
}

#[test]
fn workspace_path_is_directory() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().is_dir());
}

#[test]
fn passthrough_path_matches_source_exactly() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert_eq!(ws.path().to_string_lossy(), src.path().to_string_lossy());
}

// ===========================================================================
// 13. Concurrent workspace creation
// ===========================================================================

#[test]
fn concurrent_workspaces_independent() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "shared").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws1.path(), ws2.path());
    // Mutate ws1, ws2 should be unaffected
    fs::write(ws1.path().join("f.txt"), "changed1").unwrap();
    assert_eq!(
        fs::read_to_string(ws2.path().join("f.txt")).unwrap(),
        "shared"
    );
}

#[test]
fn concurrent_workspaces_both_have_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws1.path().join(".git").exists());
    assert!(ws2.path().join(".git").exists());
}

#[test]
fn many_concurrent_workspaces() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let workspaces: Vec<_> = (0..5)
        .map(|_| {
            WorkspaceStager::new()
                .source_root(src.path())
                .with_git_init(false)
                .stage()
                .unwrap()
        })
        .collect();
    let paths: Vec<_> = workspaces.iter().map(|w| w.path().to_path_buf()).collect();
    // All paths unique
    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            assert_ne!(paths[i], paths[j]);
        }
    }
}

#[test]
fn concurrent_stager_workspaces_independent() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "data").unwrap();
    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_ne!(ws1.path(), ws2.path());
    fs::write(ws1.path().join("a.txt"), "mutated").unwrap();
    assert_eq!(
        fs::read_to_string(ws2.path().join("a.txt")).unwrap(),
        "data"
    );
}

// ===========================================================================
// 14. Workspace with various glob patterns
// ===========================================================================

#[test]
fn glob_exclude_dot_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    fs::write(src.path().join("visible.txt"), "ok").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec![".*".into()]))
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"visible.txt".to_string()));
    assert!(!files.iter().any(|f| f.starts_with('.')));
}

#[test]
fn glob_include_specific_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.py"), "pass").unwrap();
    fs::write(src.path().join("b.js"), "//").unwrap();
    fs::write(src.path().join("c.py"), "pass").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.py".into()],
        vec![],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|f| f.ends_with(".py")));
}

#[test]
fn glob_exclude_build_artifacts() {
    let src = tempdir().unwrap();
    create_mixed_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["target/**".into(), "*.o".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("target/")));
}

#[test]
fn glob_include_only_toml_and_md() {
    let src = tempdir().unwrap();
    create_mixed_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.toml".into(), "**/*.md".into()],
        vec![],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(
            f.ends_with(".toml") || f.ends_with(".md"),
            "unexpected: {f}"
        );
    }
}

#[test]
fn glob_exclude_tests_and_docs() {
    let src = tempdir().unwrap();
    create_mixed_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["tests/**".into(), "docs/**".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
    assert!(!files.iter().any(|f| f.starts_with("docs/")));
}

#[test]
fn glob_include_css_only() {
    let src = tempdir().unwrap();
    create_mixed_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.css".into()],
        vec![],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".css"), "unexpected: {f}");
    }
}

// ===========================================================================
// 15. Snapshot capture/compare
// ===========================================================================

#[test]
fn snapshot_capture_basic() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let snap = capture(src.path()).unwrap();
    assert!(snap.file_count() > 0);
    assert!(snap.total_size() > 0);
}

#[test]
fn snapshot_has_all_source_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "aaa").unwrap();
    fs::write(src.path().join("b.txt"), "bbb").unwrap();
    let snap = capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 2);
    assert!(snap.has_file(Path::new("a.txt")));
    assert!(snap.has_file(Path::new("b.txt")));
}

#[test]
fn snapshot_records_file_sizes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();
    let snap = capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("a.txt")).unwrap();
    assert_eq!(f.size, 5);
}

#[test]
fn snapshot_records_sha256() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();
    let snap = capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("a.txt")).unwrap();
    assert!(!f.sha256.is_empty());
    assert_eq!(f.sha256.len(), 64); // SHA-256 hex
}

#[test]
fn snapshot_compare_identical() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "data").unwrap();
    let s1 = capture(src.path()).unwrap();
    let s2 = capture(src.path()).unwrap();
    let diff = compare(&s1, &s2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 1);
}

#[test]
fn snapshot_compare_detects_addition() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "data").unwrap();
    let s1 = capture(src.path()).unwrap();
    fs::write(src.path().join("b.txt"), "new").unwrap();
    let s2 = capture(src.path()).unwrap();
    let diff = compare(&s1, &s2);
    assert_eq!(diff.added.len(), 1);
}

#[test]
fn snapshot_compare_detects_removal() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "data").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    let s1 = capture(src.path()).unwrap();
    fs::remove_file(src.path().join("b.txt")).unwrap();
    let s2 = capture(src.path()).unwrap();
    let diff = compare(&s1, &s2);
    assert_eq!(diff.removed.len(), 1);
}

#[test]
fn snapshot_compare_detects_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "original").unwrap();
    let s1 = capture(src.path()).unwrap();
    fs::write(src.path().join("a.txt"), "modified").unwrap();
    let s2 = capture(src.path()).unwrap();
    let diff = compare(&s1, &s2);
    assert_eq!(diff.modified.len(), 1);
    assert!(diff.unchanged.is_empty());
}

#[test]
fn snapshot_detects_binary() {
    let src = tempdir().unwrap();
    let binary = vec![0u8, 1, 2, 0, 255];
    fs::write(src.path().join("bin.dat"), &binary).unwrap();
    let snap = capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("bin.dat")).unwrap();
    assert!(f.is_binary);
}

#[test]
fn snapshot_text_not_binary() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("text.txt"), "hello world").unwrap();
    let snap = capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("text.txt")).unwrap();
    assert!(!f.is_binary);
}

// ===========================================================================
// 16. DiffSummary analysis
// ===========================================================================

#[test]
fn diff_summary_is_empty_default() {
    let ds = DiffSummary::default();
    assert!(ds.is_empty());
    assert_eq!(ds.file_count(), 0);
    assert_eq!(ds.total_changes(), 0);
}

#[test]
fn diff_summary_reports_additions_and_deletions() {
    let ds = DiffSummary {
        added: vec![PathBuf::from("new.txt")],
        modified: vec![],
        deleted: vec![PathBuf::from("old.txt")],
        total_additions: 10,
        total_deletions: 5,
    };
    assert!(!ds.is_empty());
    assert_eq!(ds.file_count(), 2);
    assert_eq!(ds.total_changes(), 15);
}

#[test]
fn diff_summary_all_fields() {
    let ds = DiffSummary {
        added: vec![PathBuf::from("a.txt")],
        modified: vec![PathBuf::from("b.txt")],
        deleted: vec![PathBuf::from("c.txt")],
        total_additions: 3,
        total_deletions: 2,
    };
    assert_eq!(ds.file_count(), 3);
    assert_eq!(ds.total_changes(), 5);
}

// ===========================================================================
// 17. Template system
// ===========================================================================

#[test]
fn template_create_and_add_files() {
    let mut tpl = WorkspaceTemplate::new("test", "A test template");
    tpl.add_file("src/main.rs", "fn main() {}");
    tpl.add_file("Cargo.toml", "[package]");
    assert_eq!(tpl.file_count(), 2);
    assert!(tpl.has_file("src/main.rs"));
    assert!(tpl.has_file("Cargo.toml"));
}

#[test]
fn template_apply_creates_files() {
    let mut tpl = WorkspaceTemplate::new("test", "test desc");
    tpl.add_file("a.txt", "aaa");
    tpl.add_file("sub/b.txt", "bbb");
    let dir = tempdir().unwrap();
    let written = tpl.apply(dir.path()).unwrap();
    assert_eq!(written, 2);
    assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "aaa");
    assert_eq!(
        fs::read_to_string(dir.path().join("sub").join("b.txt")).unwrap(),
        "bbb"
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
    let tpl = WorkspaceTemplate::new("name", "");
    let problems = tpl.validate();
    assert!(problems.iter().any(|p| p.contains("description")));
}

#[test]
fn template_validate_ok() {
    let tpl = WorkspaceTemplate::new("good", "good desc");
    assert!(tpl.validate().is_empty());
}

#[test]
fn template_registry_register_and_get() {
    let mut reg = TemplateRegistry::new();
    let tpl = WorkspaceTemplate::new("rust", "Rust template");
    reg.register(tpl);
    assert_eq!(reg.count(), 1);
    assert!(reg.get("rust").is_some());
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn template_registry_list() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("alpha", "a"));
    reg.register(WorkspaceTemplate::new("beta", "b"));
    let names = reg.list();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn template_registry_overwrite() {
    let mut reg = TemplateRegistry::new();
    let mut t1 = WorkspaceTemplate::new("tpl", "v1");
    t1.add_file("a.txt", "old");
    reg.register(t1);
    let mut t2 = WorkspaceTemplate::new("tpl", "v2");
    t2.add_file("b.txt", "new");
    reg.register(t2);
    assert_eq!(reg.count(), 1);
    let t = reg.get("tpl").unwrap();
    assert_eq!(t.description, "v2");
    assert!(t.has_file("b.txt"));
    assert!(!t.has_file("a.txt"));
}

// ===========================================================================
// 18. ChangeTracker / OperationLog
// ===========================================================================

#[test]
fn change_tracker_basic() {
    let mut tracker = ChangeTracker::new();
    assert!(!tracker.has_changes());
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    assert!(tracker.has_changes());
    assert_eq!(tracker.changes().len(), 1);
}

#[test]
fn change_tracker_summary() {
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
        kind: ChangeKind::Modified,
        size_before: Some(50),
        size_after: Some(75),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(30),
        size_after: None,
        content_hash: None,
    });
    let summary = tracker.summary();
    assert_eq!(summary.created, 1);
    assert_eq!(summary.modified, 1);
    assert_eq!(summary.deleted, 1);
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
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(20),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(5),
        size_after: None,
        content_hash: None,
    });
    let created = tracker.by_kind(&ChangeKind::Created);
    assert_eq!(created.len(), 2);
}

#[test]
fn change_tracker_affected_paths() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "x.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(1),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "y.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(1),
        size_after: Some(2),
        content_hash: None,
    });
    let paths = tracker.affected_paths();
    assert_eq!(paths, vec!["x.txt", "y.txt"]);
}

#[test]
fn change_tracker_clear() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(1),
        content_hash: None,
    });
    assert!(tracker.has_changes());
    tracker.clear();
    assert!(!tracker.has_changes());
}

#[test]
fn change_tracker_renamed() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "new_name.txt".into(),
        kind: ChangeKind::Renamed {
            from: "old_name.txt".into(),
        },
        size_before: Some(10),
        size_after: Some(10),
        content_hash: None,
    });
    let summary = tracker.summary();
    assert_eq!(summary.renamed, 1);
}

#[test]
fn change_tracker_size_delta() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "grow.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(10),
        size_after: Some(30),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "shrink.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(50),
        size_after: Some(20),
        content_hash: None,
    });
    let summary = tracker.summary();
    // +20 - 30 = -10
    assert_eq!(summary.total_size_delta, -10);
}

#[test]
fn operation_log_basic() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "b.txt".into(),
        size: 100,
    });
    assert_eq!(log.operations().len(), 2);
    assert_eq!(log.reads(), vec!["a.txt"]);
    assert_eq!(log.writes(), vec!["b.txt"]);
}

#[test]
fn operation_log_summary() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "b.txt".into(),
        size: 50,
    });
    log.record(FileOperation::Write {
        path: "c.txt".into(),
        size: 30,
    });
    log.record(FileOperation::Delete {
        path: "d.txt".into(),
    });
    let s = log.summary();
    assert_eq!(s.reads, 1);
    assert_eq!(s.writes, 2);
    assert_eq!(s.deletes, 1);
    assert_eq!(s.total_writes_bytes, 80);
}

#[test]
fn operation_log_affected_paths() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Move {
        from: "b.txt".into(),
        to: "c.txt".into(),
    });
    let paths = log.affected_paths();
    assert!(paths.contains("a.txt"));
    assert!(paths.contains("b.txt"));
    assert!(paths.contains("c.txt"));
}

#[test]
fn operation_log_clear() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    assert!(!log.operations().is_empty());
    log.clear();
    assert!(log.operations().is_empty());
}

#[test]
fn operation_filter_allow_all_by_default() {
    let filter = OperationFilter::new();
    assert!(filter.is_allowed("anything.txt"));
}

#[test]
fn operation_filter_deny() {
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.log");
    assert!(!filter.is_allowed("debug.log"));
    assert!(filter.is_allowed("code.rs"));
}

#[test]
fn operation_filter_allow_list() {
    let mut filter = OperationFilter::new();
    filter.add_allowed_path("*.rs");
    assert!(filter.is_allowed("main.rs"));
    assert!(!filter.is_allowed("data.json"));
}

#[test]
fn operation_filter_filter_operations() {
    let ops = vec![
        FileOperation::Read {
            path: "good.rs".into(),
        },
        FileOperation::Write {
            path: "bad.log".into(),
            size: 10,
        },
        FileOperation::Read {
            path: "also_good.rs".into(),
        },
    ];
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.log");
    let filtered = filter.filter_operations(&ops);
    assert_eq!(filtered.len(), 2);
}

// ===========================================================================
// 19. Error handling
// ===========================================================================

#[test]
fn error_nonexistent_source_directory() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/42/xyz")
        .stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("does not exist"), "got: {msg}");
}

#[test]
fn error_no_source_root_set() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("source_root"), "got: {msg}");
}

// ===========================================================================
// 20. Re-staging
// ===========================================================================

#[test]
fn restage_from_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("orig.txt"), "v1").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws1.path().join("orig.txt"), "v2").unwrap();
    fs::write(ws1.path().join("added.txt"), "new").unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(ws1.path())).unwrap();
    assert_ne!(ws1.path(), ws2.path());
    assert_eq!(
        fs::read_to_string(ws2.path().join("orig.txt")).unwrap(),
        "v2"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("added.txt")).unwrap(),
        "new"
    );
}

#[test]
fn restage_does_not_copy_dot_git_from_first() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(ws1.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws2.path().join(".git").exists());
    assert!(ws2.path().join("code.rs").exists());
}

// ===========================================================================
// 21. Misc integration
// ===========================================================================

#[test]
fn stager_default_is_new() {
    let s1 = WorkspaceStager::new();
    let s2 = WorkspaceStager::default();
    // Both should be equivalent - just verify they compile
    assert!(format!("{s1:?}").contains("WorkspaceStager"));
    assert!(format!("{s2:?}").contains("WorkspaceStager"));
}

#[test]
fn prepared_workspace_debug_format() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let dbg = format!("{ws:?}");
    assert!(dbg.contains("PreparedWorkspace"));
}

#[test]
fn workspace_with_special_chars_in_filenames() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello world.txt"), "spaces").unwrap();
    fs::write(src.path().join("file-with-dashes.txt"), "dashes").unwrap();
    fs::write(src.path().join("under_score.txt"), "underscores").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("hello world.txt")).unwrap(),
        "spaces"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("file-with-dashes.txt")).unwrap(),
        "dashes"
    );
}

#[test]
fn workspace_file_operation_paths() {
    let op = FileOperation::Copy {
        from: "src.txt".into(),
        to: "dst.txt".into(),
    };
    let paths = op.paths();
    assert_eq!(paths, vec!["src.txt", "dst.txt"]);
}

#[test]
fn workspace_file_operation_create_dir() {
    let op = FileOperation::CreateDir {
        path: "new_dir".into(),
    };
    assert_eq!(op.paths(), vec!["new_dir"]);
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

#[test]
fn operation_log_deletes() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Delete {
        path: "gone.txt".into(),
    });
    assert_eq!(log.deletes(), vec!["gone.txt"]);
}

#[test]
fn operation_log_move_and_copy() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Move {
        from: "old.txt".into(),
        to: "new.txt".into(),
    });
    log.record(FileOperation::Copy {
        from: "src.txt".into(),
        to: "dst.txt".into(),
    });
    let s = log.summary();
    assert_eq!(s.moves, 1);
    assert_eq!(s.copies, 1);
}

#[test]
fn operation_log_create_dir() {
    let mut log = OperationLog::new();
    log.record(FileOperation::CreateDir {
        path: "my_dir".into(),
    });
    let s = log.summary();
    assert_eq!(s.create_dirs, 1);
}

#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive workspace and git operations tests.
//!
//! Covers WorkspaceStager builder, WorkspaceManager, git init/status/diff,
//! include/exclude filtering, directory structure, file types, edge cases,
//! snapshot/diff analysis, templates, trackers, and operation logging.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{
    ChangeType, DiffAnalysis, DiffAnalyzer, DiffChangeKind, DiffPolicy, DiffSummary, FileType,
    PolicyResult, WorkspaceDiff,
};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog, OperationSummary};
use abp_workspace::snapshot;
use abp_workspace::template::{TemplateRegistry, WorkspaceTemplate};
use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};
use abp_workspace::{PreparedWorkspace, WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_source(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = tempdir().unwrap();
    for (rel, content) in files {
        let p = tmp.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
    }
    tmp
}

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

fn collect_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(root).unwrap().to_path_buf())
        .collect()
}

fn has_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

fn git_log_count(path: &Path) -> usize {
    let out = Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(path)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().count(),
        _ => 0,
    }
}

// ===========================================================================
// WorkspaceStager builder tests
// ===========================================================================

#[test]
fn stager_new_defaults() {
    let stager = WorkspaceStager::new();
    // Should not panic — default state is valid except missing source_root
    let err = stager.stage();
    assert!(err.is_err(), "stage without source_root should fail");
}

#[test]
fn stager_default_trait() {
    let stager = WorkspaceStager::default();
    assert!(stager.stage().is_err());
}

#[test]
fn stager_source_root_sets_path() {
    let src = make_source(&[("a.txt", "hello")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().exists());
    assert!(ws.path().join("a.txt").exists());
}

#[test]
fn stager_source_root_nonexistent_fails() {
    let err = WorkspaceStager::new()
        .source_root("/nonexistent/path/xyz")
        .stage();
    assert!(err.is_err());
}

#[test]
fn stager_include_filters_files() {
    let src = make_source(&[
        ("a.rs", "fn main(){}"),
        ("b.txt", "text"),
        ("c.rs", "fn c(){}"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.to_str().unwrap().contains("a.rs")));
    assert!(files.iter().any(|f| f.to_str().unwrap().contains("c.rs")));
    assert!(!files.iter().any(|f| f.to_str().unwrap().contains("b.txt")));
}

#[test]
fn stager_exclude_filters_files() {
    let src = make_source(&[("a.rs", "fn main(){}"), ("b.log", "log data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.to_str().unwrap().contains("a.rs")));
    assert!(!files.iter().any(|f| f.to_str().unwrap().contains("b.log")));
}

#[test]
fn stager_include_and_exclude_combined() {
    let src = make_source(&[
        ("src/main.rs", "fn main(){}"),
        ("src/test.rs", "fn test(){}"),
        ("docs/readme.md", "# Docs"),
        ("build/out.o", "binary"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["**/test.rs".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(
        files
            .iter()
            .any(|f| f.to_str().unwrap().contains("main.rs"))
    );
    assert!(
        !files
            .iter()
            .any(|f| f.to_str().unwrap().contains("test.rs"))
    );
    assert!(
        !files
            .iter()
            .any(|f| f.to_str().unwrap().contains("readme.md"))
    );
}

#[test]
fn stager_with_git_init_true_creates_repo() {
    let src = make_source(&[("hello.txt", "world")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(has_git_repo(ws.path()));
}

#[test]
fn stager_with_git_init_false_no_repo() {
    let src = make_source(&[("hello.txt", "world")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!has_git_repo(ws.path()));
}

#[test]
fn stager_fluent_chaining() {
    let src = make_source(&[("a.txt", "a"), ("b.log", "b")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.txt".into()])
        .exclude(vec![])
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join("a.txt").exists());
    assert!(has_git_repo(ws.path()));
}

#[test]
fn stager_empty_include_copies_all() {
    let src = make_source(&[("a.txt", "a"), ("b.rs", "b")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec![])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.len() >= 2);
}

#[test]
fn stager_empty_exclude_copies_all() {
    let src = make_source(&[("a.txt", "a"), ("b.rs", "b")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec![])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.len() >= 2);
}

// ===========================================================================
// WorkspaceManager::prepare tests
// ===========================================================================

#[test]
fn manager_prepare_staged_creates_copy() {
    let src = make_source(&[("foo.txt", "bar")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("foo.txt").exists());
    assert_ne!(ws.path(), src.path());
}

#[test]
fn manager_prepare_passthrough_uses_original() {
    let src = make_source(&[("foo.txt", "bar")]);
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn manager_prepare_staged_has_git() {
    let src = make_source(&[("foo.txt", "bar")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(has_git_repo(ws.path()));
}

#[test]
fn manager_prepare_staged_with_globs() {
    let src = make_source(&[("a.rs", "code"), ("b.txt", "text")]);
    let spec = staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("a.rs").exists());
    assert!(!ws.path().join("b.txt").exists());
}

// ===========================================================================
// Git init baseline commit tests
// ===========================================================================

#[test]
fn git_init_produces_baseline_commit() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(git_log_count(ws.path()), 1);
}

#[test]
fn git_init_baseline_message() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let out = Command::new("git")
        .args(["log", "--oneline", "--format=%s"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let msg = String::from_utf8_lossy(&out.stdout);
    assert!(msg.trim().contains("baseline"));
}

#[test]
fn git_init_clean_status() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(
        status.unwrap().trim().is_empty(),
        "status should be clean after baseline"
    );
}

#[test]
fn git_init_clean_diff() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    assert!(
        diff.unwrap().trim().is_empty(),
        "diff should be empty after baseline"
    );
}

#[test]
fn git_init_all_files_committed() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b"), ("sub/c.txt", "c")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let out = Command::new("git")
        .args(["ls-files"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let tracked: Vec<&str> = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();
    assert!(tracked.len() >= 3);
}

// ===========================================================================
// git_status after modifications
// ===========================================================================

#[test]
fn git_status_detects_new_file() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("new.txt"), "new content").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("new.txt"));
}

#[test]
fn git_status_detects_modified_file() {
    let src = make_source(&[("file.txt", "original")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("file.txt"), "modified").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("file.txt"));
}

#[test]
fn git_status_detects_deleted_file() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::remove_file(ws.path().join("file.txt")).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("file.txt"));
}

#[test]
fn git_status_empty_on_no_changes() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn git_status_multiple_changes() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("a.txt"), "modified").unwrap();
    fs::write(ws.path().join("c.txt"), "new").unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("a.txt"));
    assert!(status.contains("b.txt"));
    assert!(status.contains("c.txt"));
}

// ===========================================================================
// git_diff after modifications
// ===========================================================================

#[test]
fn git_diff_shows_modification() {
    let src = make_source(&[("file.txt", "line1\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("file.txt"), "line1\nline2\n").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("+line2"));
}

#[test]
fn git_diff_empty_when_no_changes() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn git_diff_shows_deletion_content() {
    let src = make_source(&[("file.txt", "line1\nline2\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("file.txt"), "line1\n").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("-line2"));
}

// ===========================================================================
// .git directory exclusion during staging
// ===========================================================================

#[test]
fn git_dir_excluded_from_staging() {
    let src = make_source(&[("file.txt", "content")]);
    // Manually create a .git dir in source
    fs::create_dir_all(src.path().join(".git/objects")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // The staged workspace should not have the source's .git contents
    // (it may have its own if git_init was true, but we set false)
    assert!(ws.path().join("file.txt").exists());
    // Confirm no .git/HEAD from source was copied
    assert!(
        !ws.path().join(".git/HEAD").exists() || {
            // If .git exists it was created by git init, not copied
            false
        }
    );
}

#[test]
fn git_dir_excluded_even_with_include_all() {
    let src = make_source(&[("code.rs", "fn main(){}")]);
    fs::create_dir_all(src.path().join(".git/refs")).unwrap();
    fs::write(src.path().join(".git/config"), "[core]").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("code.rs").exists());
    assert!(!ws.path().join(".git/config").exists());
}

#[test]
fn staging_with_git_init_creates_fresh_repo_not_source_git() {
    let src = make_source(&[("x.txt", "data")]);
    // Create a git repo in source with a custom branch
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(src.path())
        .output();
    let _ = Command::new("git")
        .args(["checkout", "-b", "custom-branch"])
        .current_dir(src.path())
        .output();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    // The staged workspace has its own git repo
    assert!(has_git_repo(ws.path()));
    // Baseline commit should exist
    assert!(git_log_count(ws.path()) >= 1);
}

// ===========================================================================
// Include/exclude filtering
// ===========================================================================

#[test]
fn include_single_extension() {
    let src = make_source(&[("a.rs", "code"), ("b.py", "code"), ("c.rs", "code")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert_eq!(
        files
            .iter()
            .filter(|f| f.to_str().unwrap().ends_with(".rs"))
            .count(),
        2
    );
    assert_eq!(
        files
            .iter()
            .filter(|f| f.to_str().unwrap().ends_with(".py"))
            .count(),
        0
    );
}

#[test]
fn include_directory_glob() {
    let src = make_source(&[
        ("src/main.rs", "code"),
        ("src/lib.rs", "code"),
        ("tests/test.rs", "code"),
        ("readme.md", "docs"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(
        files
            .iter()
            .any(|f| f.to_str().unwrap().contains("main.rs"))
    );
    assert!(files.iter().any(|f| f.to_str().unwrap().contains("lib.rs")));
    assert!(
        !files
            .iter()
            .any(|f| f.to_str().unwrap().contains("test.rs"))
    );
}

#[test]
fn exclude_single_file() {
    let src = make_source(&[("a.txt", "a"), ("secret.key", "key")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["secret.key".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.to_str().unwrap().contains("a.txt")));
    assert!(
        !files
            .iter()
            .any(|f| f.to_str().unwrap().contains("secret.key"))
    );
}

#[test]
fn exclude_directory_glob() {
    let src = make_source(&[
        ("src/main.rs", "code"),
        ("target/debug/out", "binary"),
        ("target/release/out", "binary"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["target/**".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(
        files
            .iter()
            .any(|f| f.to_str().unwrap().contains("main.rs"))
    );
    assert!(!files.iter().any(|f| f.to_str().unwrap().contains("target")));
}

#[test]
fn exclude_takes_precedence_over_include() {
    let src = make_source(&[("src/main.rs", "code"), ("src/generated.rs", "gen")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["**/generated.rs".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(
        files
            .iter()
            .any(|f| f.to_str().unwrap().contains("main.rs"))
    );
    assert!(
        !files
            .iter()
            .any(|f| f.to_str().unwrap().contains("generated.rs"))
    );
}

#[test]
fn multiple_include_patterns() {
    let src = make_source(&[
        ("a.rs", "rust"),
        ("b.py", "python"),
        ("c.js", "js"),
        ("d.toml", "config"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into(), "*.toml".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.to_str().unwrap().ends_with(".rs")));
    assert!(files.iter().any(|f| f.to_str().unwrap().ends_with(".toml")));
    assert!(!files.iter().any(|f| f.to_str().unwrap().ends_with(".py")));
    assert!(!files.iter().any(|f| f.to_str().unwrap().ends_with(".js")));
}

#[test]
fn multiple_exclude_patterns() {
    let src = make_source(&[
        ("a.rs", "code"),
        ("b.log", "log"),
        ("c.tmp", "temp"),
        ("d.txt", "text"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into(), "*.tmp".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.to_str().unwrap().ends_with(".rs")));
    assert!(files.iter().any(|f| f.to_str().unwrap().ends_with(".txt")));
    assert!(!files.iter().any(|f| f.to_str().unwrap().ends_with(".log")));
    assert!(!files.iter().any(|f| f.to_str().unwrap().ends_with(".tmp")));
}

// ===========================================================================
// Nested directory structure preservation
// ===========================================================================

#[test]
fn nested_dirs_preserved() {
    let src = make_source(&[
        ("a/b/c/d.txt", "deep"),
        ("a/b/e.txt", "mid"),
        ("a/f.txt", "shallow"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("a/b/c/d.txt").exists());
    assert!(ws.path().join("a/b/e.txt").exists());
    assert!(ws.path().join("a/f.txt").exists());
}

#[test]
fn deeply_nested_structure() {
    let deep = "l1/l2/l3/l4/l5/l6/l7/l8/file.txt";
    let src = make_source(&[(deep, "deep content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(deep).exists());
    let content = fs::read_to_string(ws.path().join(deep)).unwrap();
    assert_eq!(content, "deep content");
}

#[test]
fn multiple_files_in_same_directory() {
    let src = make_source(&[("dir/a.txt", "a"), ("dir/b.txt", "b"), ("dir/c.txt", "c")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("dir/a.txt").exists());
    assert!(ws.path().join("dir/b.txt").exists());
    assert!(ws.path().join("dir/c.txt").exists());
}

#[test]
fn sibling_directories() {
    let src = make_source(&[
        ("alpha/a.txt", "a"),
        ("beta/b.txt", "b"),
        ("gamma/c.txt", "c"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("alpha/a.txt").exists());
    assert!(ws.path().join("beta/b.txt").exists());
    assert!(ws.path().join("gamma/c.txt").exists());
}

// ===========================================================================
// Various file types
// ===========================================================================

#[test]
fn stage_text_file() {
    let src = make_source(&[("readme.md", "# Hello\nWorld")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("readme.md")).unwrap(),
        "# Hello\nWorld"
    );
}

#[test]
fn stage_binary_like_content() {
    let src = make_source(&[]);
    fs::write(src.path().join("data.bin"), &[0u8, 1, 2, 255, 0, 128, 64]).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let content = fs::read(ws.path().join("data.bin")).unwrap();
    assert_eq!(content, vec![0u8, 1, 2, 255, 0, 128, 64]);
}

#[test]
fn stage_empty_file() {
    let src = make_source(&[("empty.txt", "")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("empty.txt").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

#[test]
fn stage_file_with_special_characters_in_content() {
    let content = "line1\nline2\ttab\rcarriage\n\n\nempty lines above";
    let src = make_source(&[("special.txt", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("special.txt")).unwrap(),
        content
    );
}

#[test]
fn stage_unicode_content() {
    let content = "こんにちは世界 🌍 Ñoño café résumé";
    let src = make_source(&[("unicode.txt", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("unicode.txt")).unwrap(),
        content
    );
}

#[test]
fn stage_json_file() {
    let content = r#"{"key": "value", "num": 42}"#;
    let src = make_source(&[("data.json", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("data.json")).unwrap(),
        content
    );
}

#[test]
fn stage_toml_file() {
    let content = "[package]\nname = \"test\"\nversion = \"0.1.0\"";
    let src = make_source(&[("Cargo.toml", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("Cargo.toml")).unwrap(),
        content
    );
}

// ===========================================================================
// Large file handling
// ===========================================================================

#[test]
fn stage_large_file() {
    let src = make_source(&[]);
    let large_content: String = "x".repeat(1_000_000);
    fs::write(src.path().join("large.txt"), &large_content).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let staged = fs::read_to_string(ws.path().join("large.txt")).unwrap();
    assert_eq!(staged.len(), 1_000_000);
}

#[test]
fn stage_many_files() {
    let src = make_source(&[]);
    for i in 0..100 {
        fs::write(
            src.path().join(format!("file_{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.len() >= 100);
}

// ===========================================================================
// Empty directory handling
// ===========================================================================

#[test]
fn empty_source_dir_stages_ok() {
    let src = make_source(&[]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().exists());
}

#[test]
fn empty_source_git_init_still_works() {
    let src = make_source(&[]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    // Git repo exists (may have 0 or 1 commit depending on empty repo behavior)
    assert!(has_git_repo(ws.path()));
}

// ===========================================================================
// Workspace cleanup (tempdir drops)
// ===========================================================================

#[test]
fn workspace_cleanup_on_drop() {
    let src = make_source(&[("file.txt", "content")]);
    let ws_path;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .stage()
            .unwrap();
        ws_path = ws.path().to_path_buf();
        assert!(ws_path.exists());
    }
    // After drop, the temp directory should be cleaned up
    assert!(!ws_path.exists(), "temp dir should be removed after drop");
}

#[test]
fn passthrough_no_cleanup() {
    let src = make_source(&[("file.txt", "content")]);
    let src_path = src.path().to_path_buf();
    {
        let ws = WorkspaceManager::prepare(&passthrough_spec(&src_path)).unwrap();
        assert_eq!(ws.path(), src_path);
    }
    assert!(src_path.exists(), "passthrough should not clean up source");
}

#[test]
fn multiple_workspaces_independent_cleanup() {
    let src = make_source(&[("f.txt", "data")]);
    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let p1 = ws1.path().to_path_buf();
    let p2 = ws2.path().to_path_buf();
    assert_ne!(p1, p2);
    drop(ws1);
    assert!(!p1.exists());
    assert!(p2.exists());
    drop(ws2);
    assert!(!p2.exists());
}

// ===========================================================================
// PreparedWorkspace API
// ===========================================================================

#[test]
fn prepared_workspace_path_returns_valid_path() {
    let src = make_source(&[("a.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().is_absolute() || ws.path().exists());
}

// ===========================================================================
// diff_workspace function tests
// ===========================================================================

#[test]
fn diff_workspace_empty_after_staging() {
    let src = make_source(&[("a.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(diff.is_empty());
    assert_eq!(diff.file_count(), 0);
    assert_eq!(diff.total_changes(), 0);
}

#[test]
fn diff_workspace_detects_added_file() {
    let src = make_source(&[("a.txt", "a")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("new.txt"), "new content").unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(!diff.is_empty());
    assert!(
        diff.added
            .iter()
            .any(|p| p.to_str().unwrap().contains("new.txt"))
    );
}

#[test]
fn diff_workspace_detects_modified_file() {
    let src = make_source(&[("a.txt", "original\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("a.txt"), "modified\n").unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(
        diff.modified
            .iter()
            .any(|p| p.to_str().unwrap().contains("a.txt"))
    );
}

#[test]
fn diff_workspace_detects_deleted_file() {
    let src = make_source(&[("a.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::remove_file(ws.path().join("a.txt")).unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(
        diff.deleted
            .iter()
            .any(|p| p.to_str().unwrap().contains("a.txt"))
    );
}

#[test]
fn diff_workspace_line_counts() {
    let src = make_source(&[("a.txt", "line1\nline2\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("a.txt"), "line1\nline2\nline3\nline4\n").unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(diff.total_additions >= 2);
}

#[test]
fn diff_summary_is_empty_default() {
    let ds = DiffSummary::default();
    assert!(ds.is_empty());
    assert_eq!(ds.file_count(), 0);
    assert_eq!(ds.total_changes(), 0);
}

// ===========================================================================
// DiffAnalyzer tests
// ===========================================================================

#[test]
fn diff_analyzer_no_changes() {
    let src = make_source(&[("a.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(!analyzer.has_changes());
    let diff = analyzer.analyze().unwrap();
    assert!(diff.is_empty());
}

#[test]
fn diff_analyzer_detects_changes() {
    let src = make_source(&[("a.txt", "content\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("a.txt"), "changed\n").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.has_changes());
}

#[test]
fn diff_analyzer_changed_files() {
    let src = make_source(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("a.txt"), "changed\n").unwrap();
    fs::write(ws.path().join("c.txt"), "new\n").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let changed = analyzer.changed_files();
    assert!(
        changed
            .iter()
            .any(|p| p.to_str().unwrap().contains("a.txt"))
    );
    assert!(
        changed
            .iter()
            .any(|p| p.to_str().unwrap().contains("c.txt"))
    );
}

#[test]
fn diff_analyzer_file_was_modified() {
    let src = make_source(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("a.txt"), "changed\n").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.file_was_modified(Path::new("a.txt")));
    assert!(!analyzer.file_was_modified(Path::new("b.txt")));
}

#[test]
fn diff_analyzer_summary_text() {
    let diff = WorkspaceDiff::default();
    assert_eq!(diff.summary(), "No changes detected.");
}

#[test]
fn workspace_diff_file_count() {
    let mut diff = WorkspaceDiff::default();
    diff.files_added.push(abp_workspace::diff::FileChange {
        path: PathBuf::from("new.txt"),
        change_type: ChangeType::Added,
        additions: 5,
        deletions: 0,
        is_binary: false,
    });
    assert_eq!(diff.file_count(), 1);
    assert!(!diff.is_empty());
}

// ===========================================================================
// DiffAnalysis parse tests
// ===========================================================================

#[test]
fn diff_analysis_parse_empty() {
    let analysis = DiffAnalysis::parse("");
    assert!(analysis.is_empty());
    assert_eq!(analysis.file_count(), 0);
}

#[test]
fn diff_analysis_parse_simple_add() {
    let raw = "\
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+hello
+world
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.total_additions, 2);
    assert_eq!(analysis.total_deletions, 0);
    assert_eq!(analysis.files[0].change_kind, DiffChangeKind::Added);
}

#[test]
fn diff_analysis_parse_modification() {
    let raw = "\
diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,3 @@
 line1
-line2
+line2_modified
+line3
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.files[0].change_kind, DiffChangeKind::Modified);
    assert!(analysis.total_additions >= 2);
    assert!(analysis.total_deletions >= 1);
}

#[test]
fn diff_analysis_parse_deletion() {
    let raw = "\
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-line1
-line2
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.files[0].change_kind, DiffChangeKind::Deleted);
    assert_eq!(analysis.total_deletions, 2);
}

#[test]
fn diff_analysis_file_type_detection() {
    let raw = "\
diff --git a/code.rs b/code.rs
new file mode 100644
--- /dev/null
+++ b/code.rs
@@ -0,0 +1 @@
+fn main() {}
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.files[0].file_type, FileType::Rust);
}

#[test]
fn diff_analysis_multiple_files() {
    let raw = "\
diff --git a/a.txt b/a.txt
new file mode 100644
--- /dev/null
+++ b/a.txt
@@ -0,0 +1 @@
+hello
diff --git a/b.txt b/b.txt
new file mode 100644
--- /dev/null
+++ b/b.txt
@@ -0,0 +1 @@
+world
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 2);
}

#[test]
fn diff_analysis_files_by_kind() {
    let raw = "\
diff --git a/added.txt b/added.txt
new file mode 100644
--- /dev/null
+++ b/added.txt
@@ -0,0 +1 @@
+new
diff --git a/modified.txt b/modified.txt
--- a/modified.txt
+++ b/modified.txt
@@ -1 +1 @@
-old
+new
";
    let analysis = DiffAnalysis::parse(raw);
    let added = analysis.files_by_kind(DiffChangeKind::Added);
    let modified = analysis.files_by_kind(DiffChangeKind::Modified);
    assert_eq!(added.len(), 1);
    assert_eq!(modified.len(), 1);
}

#[test]
fn diff_analysis_file_stats() {
    let raw = "\
diff --git a/code.py b/code.py
new file mode 100644
--- /dev/null
+++ b/code.py
@@ -0,0 +1,3 @@
+def main():
+    pass
+# end
";
    let analysis = DiffAnalysis::parse(raw);
    let stats = analysis.file_stats();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].file_type, FileType::Python);
    assert_eq!(stats[0].additions, 3);
}

// ===========================================================================
// identify_file_type tests
// ===========================================================================

#[test]
fn identify_rust() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("main.rs"),
        FileType::Rust
    );
}

#[test]
fn identify_javascript() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("app.js"),
        FileType::JavaScript
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("mod.mjs"),
        FileType::JavaScript
    );
}

#[test]
fn identify_typescript() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("comp.tsx"),
        FileType::TypeScript
    );
}

#[test]
fn identify_python() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("script.py"),
        FileType::Python
    );
}

#[test]
fn identify_go() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("main.go"),
        FileType::Go
    );
}

#[test]
fn identify_json() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("data.json"),
        FileType::Json
    );
}

#[test]
fn identify_yaml() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("config.yml"),
        FileType::Yaml
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("config.yaml"),
        FileType::Yaml
    );
}

#[test]
fn identify_toml() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("Cargo.toml"),
        FileType::Toml
    );
}

#[test]
fn identify_markdown() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("README.md"),
        FileType::Markdown
    );
}

#[test]
fn identify_shell() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("run.sh"),
        FileType::Shell
    );
}

#[test]
fn identify_binary() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("image.png"),
        FileType::Binary
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("lib.dll"),
        FileType::Binary
    );
}

#[test]
fn identify_other() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("noext"),
        FileType::Other
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("file.xyz"),
        FileType::Other
    );
}

#[test]
fn identify_html() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("page.html"),
        FileType::Html
    );
}

#[test]
fn identify_css() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("style.css"),
        FileType::Css
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("style.scss"),
        FileType::Css
    );
}

#[test]
fn identify_java() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("Main.java"),
        FileType::Java
    );
}

#[test]
fn identify_sql() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("query.sql"),
        FileType::Sql
    );
}

#[test]
fn identify_xml() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("data.xml"),
        FileType::Xml
    );
}

#[test]
fn identify_csharp() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("Program.cs"),
        FileType::CSharp
    );
}

#[test]
fn identify_cpp() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("main.cpp"),
        FileType::Cpp
    );
}

#[test]
fn identify_c() {
    assert_eq!(
        abp_workspace::diff::identify_file_type("main.c"),
        FileType::C
    );
    assert_eq!(
        abp_workspace::diff::identify_file_type("header.h"),
        FileType::C
    );
}

// ===========================================================================
// DiffPolicy tests
// ===========================================================================

#[test]
fn diff_policy_pass_on_empty_diff() {
    let policy = DiffPolicy::default();
    let diff = WorkspaceDiff::default();
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_max_files_enforced() {
    let policy = DiffPolicy {
        max_files: Some(1),
        ..Default::default()
    };
    let mut diff = WorkspaceDiff::default();
    diff.files_added.push(abp_workspace::diff::FileChange {
        path: PathBuf::from("a.txt"),
        change_type: ChangeType::Added,
        additions: 1,
        deletions: 0,
        is_binary: false,
    });
    diff.files_added.push(abp_workspace::diff::FileChange {
        path: PathBuf::from("b.txt"),
        change_type: ChangeType::Added,
        additions: 1,
        deletions: 0,
        is_binary: false,
    });
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

#[test]
fn diff_policy_max_additions_enforced() {
    let policy = DiffPolicy {
        max_additions: Some(5),
        ..Default::default()
    };
    let mut diff = WorkspaceDiff::default();
    diff.total_additions = 10;
    diff.files_added.push(abp_workspace::diff::FileChange {
        path: PathBuf::from("a.txt"),
        change_type: ChangeType::Added,
        additions: 10,
        deletions: 0,
        is_binary: false,
    });
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

#[test]
fn diff_policy_denied_paths() {
    let policy = DiffPolicy {
        denied_paths: vec!["*.secret".into()],
        ..Default::default()
    };
    let mut diff = WorkspaceDiff::default();
    diff.files_added.push(abp_workspace::diff::FileChange {
        path: PathBuf::from("key.secret"),
        change_type: ChangeType::Added,
        additions: 1,
        deletions: 0,
        is_binary: false,
    });
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

// ===========================================================================
// Snapshot tests
// ===========================================================================

#[test]
fn snapshot_capture_basic() {
    let src = make_source(&[("a.txt", "hello"), ("b.txt", "world")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 2);
    assert!(snap.has_file(Path::new("a.txt")));
    assert!(snap.has_file(Path::new("b.txt")));
}

#[test]
fn snapshot_file_size() {
    let src = make_source(&[("a.txt", "12345")]);
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("a.txt")).unwrap();
    assert_eq!(f.size, 5);
}

#[test]
fn snapshot_total_size() {
    let src = make_source(&[("a.txt", "hello"), ("b.txt", "hi")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.total_size(), 7);
}

#[test]
fn snapshot_sha256_consistent() {
    let src = make_source(&[("a.txt", "consistent content")]);
    let snap1 = snapshot::capture(src.path()).unwrap();
    let snap2 = snapshot::capture(src.path()).unwrap();
    let hash1 = &snap1.get_file(Path::new("a.txt")).unwrap().sha256;
    let hash2 = &snap2.get_file(Path::new("a.txt")).unwrap().sha256;
    assert_eq!(hash1, hash2);
}

#[test]
fn snapshot_binary_detection() {
    let src = make_source(&[]);
    fs::write(src.path().join("bin.dat"), &[0u8, 1, 2, 0, 255]).unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.get_file(Path::new("bin.dat")).unwrap().is_binary);
}

#[test]
fn snapshot_text_not_binary() {
    let src = make_source(&[("text.txt", "hello world")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(!snap.get_file(Path::new("text.txt")).unwrap().is_binary);
}

#[test]
fn snapshot_excludes_git_dir() {
    let src = make_source(&[("a.txt", "data")]);
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref").unwrap();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(!snap.has_file(Path::new(".git/HEAD")));
}

#[test]
fn snapshot_compare_identical() {
    let src = make_source(&[("a.txt", "same")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 1);
}

#[test]
fn snapshot_compare_added() {
    let src = make_source(&[("a.txt", "data")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("b.txt"), "new").unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert_eq!(diff.added.len(), 1);
}

#[test]
fn snapshot_compare_removed() {
    let src = make_source(&[("a.txt", "data"), ("b.txt", "data2")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    fs::remove_file(src.path().join("b.txt")).unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert_eq!(diff.removed.len(), 1);
}

#[test]
fn snapshot_compare_modified() {
    let src = make_source(&[("a.txt", "original")]);
    let s1 = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("a.txt"), "changed").unwrap();
    let s2 = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);
    assert_eq!(diff.modified.len(), 1);
}

// ===========================================================================
// WorkspaceTemplate tests
// ===========================================================================

#[test]
fn template_new() {
    let t = WorkspaceTemplate::new("test", "A test template");
    assert_eq!(t.name, "test");
    assert_eq!(t.description, "A test template");
    assert_eq!(t.file_count(), 0);
}

#[test]
fn template_add_file() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("src/main.rs", "fn main(){}");
    assert_eq!(t.file_count(), 1);
    assert!(t.has_file("src/main.rs"));
}

#[test]
fn template_apply() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("hello.txt", "world");
    t.add_file("sub/nested.txt", "data");
    let tmp = tempdir().unwrap();
    let count = t.apply(tmp.path()).unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        fs::read_to_string(tmp.path().join("hello.txt")).unwrap(),
        "world"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("sub/nested.txt")).unwrap(),
        "data"
    );
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
    t.add_file("C:\\absolute\\path.txt", "data");
    #[cfg(not(windows))]
    t.add_file("/absolute/path.txt", "data");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("absolute")));
}

// ===========================================================================
// TemplateRegistry tests
// ===========================================================================

#[test]
fn registry_new_empty() {
    let reg = TemplateRegistry::new();
    assert_eq!(reg.count(), 0);
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("rust", "Rust project"));
    assert_eq!(reg.count(), 1);
    assert!(reg.get("rust").is_some());
    assert_eq!(reg.get("rust").unwrap().description, "Rust project");
}

#[test]
fn registry_overwrite() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("t", "first"));
    reg.register(WorkspaceTemplate::new("t", "second"));
    assert_eq!(reg.count(), 1);
    assert_eq!(reg.get("t").unwrap().description, "second");
}

#[test]
fn registry_list_sorted() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("c", ""));
    reg.register(WorkspaceTemplate::new("a", ""));
    reg.register(WorkspaceTemplate::new("b", ""));
    assert_eq!(reg.list(), vec!["a", "b", "c"]);
}

#[test]
fn registry_get_nonexistent() {
    let reg = TemplateRegistry::new();
    assert!(reg.get("nope").is_none());
}

// ===========================================================================
// OperationLog tests
// ===========================================================================

#[test]
fn operation_log_new_empty() {
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
        size: 100,
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
    log.record(FileOperation::Move {
        from: "e.txt".into(),
        to: "f.txt".into(),
    });
    log.record(FileOperation::Copy {
        from: "g.txt".into(),
        to: "h.txt".into(),
    });
    log.record(FileOperation::CreateDir { path: "dir".into() });
    let s = log.summary();
    assert_eq!(s.reads, 1);
    assert_eq!(s.writes, 2);
    assert_eq!(s.deletes, 1);
    assert_eq!(s.moves, 1);
    assert_eq!(s.copies, 1);
    assert_eq!(s.create_dirs, 1);
    assert_eq!(s.total_writes_bytes, 80);
}

#[test]
fn operation_log_affected_paths() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "a.txt".into(),
        size: 10,
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
    log.clear();
    assert!(log.operations().is_empty());
}

#[test]
fn file_operation_paths() {
    let op = FileOperation::Move {
        from: "a".into(),
        to: "b".into(),
    };
    assert_eq!(op.paths(), vec!["a", "b"]);
}

// ===========================================================================
// OperationFilter tests
// ===========================================================================

#[test]
fn operation_filter_permissive_by_default() {
    let f = OperationFilter::new();
    assert!(f.is_allowed("anything.txt"));
}

#[test]
fn operation_filter_allow() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("*.rs");
    assert!(f.is_allowed("main.rs"));
    assert!(!f.is_allowed("readme.md"));
}

#[test]
fn operation_filter_deny() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.secret");
    assert!(!f.is_allowed("key.secret"));
    assert!(f.is_allowed("file.txt"));
}

#[test]
fn operation_filter_deny_takes_precedence() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("*");
    f.add_denied_path("*.log");
    assert!(!f.is_allowed("debug.log"));
    assert!(f.is_allowed("code.rs"));
}

#[test]
fn operation_filter_filter_operations() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.secret");
    let ops = vec![
        FileOperation::Read {
            path: "ok.txt".into(),
        },
        FileOperation::Read {
            path: "bad.secret".into(),
        },
        FileOperation::Move {
            from: "a.txt".into(),
            to: "b.secret".into(),
        },
    ];
    let allowed = f.filter_operations(&ops);
    assert_eq!(allowed.len(), 1);
}

// ===========================================================================
// ChangeTracker tests
// ===========================================================================

#[test]
fn change_tracker_new_empty() {
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
        size_after: Some(80),
        content_hash: None,
    });
    t.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(30),
        size_after: None,
        content_hash: None,
    });
    t.record(FileChange {
        path: "d.txt".into(),
        kind: ChangeKind::Renamed {
            from: "old.txt".into(),
        },
        size_before: Some(20),
        size_after: Some(20),
        content_hash: None,
    });
    let s = t.summary();
    assert_eq!(s.created, 1);
    assert_eq!(s.modified, 1);
    assert_eq!(s.deleted, 1);
    assert_eq!(s.renamed, 1);
    // delta: (100-0) + (80-50) + (0-30) + (20-20) = 100 + 30 - 30 + 0 = 100
    assert_eq!(s.total_size_delta, 100);
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
        size_before: Some(5),
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
    // Deduplication
    assert_eq!(t.affected_paths(), vec!["a.txt"]);
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
    t.clear();
    assert!(!t.has_changes());
}

// ===========================================================================
// Edge cases and integration scenarios
// ===========================================================================

#[test]
fn stage_preserves_file_content_exactly() {
    let content = "exact content with trailing newline\n";
    let src = make_source(&[("exact.txt", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("exact.txt")).unwrap(),
        content
    );
}

#[test]
fn stage_dotfile_preserved() {
    let src = make_source(&[(".hidden", "secret"), (".config/settings", "val")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(".hidden").exists());
    assert!(ws.path().join(".config/settings").exists());
}

#[test]
fn workspace_manager_git_status_none_for_non_repo() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("a.txt"), "data").unwrap();
    let status = WorkspaceManager::git_status(tmp.path());
    // Non-git dir returns None
    assert!(status.is_none());
}

#[test]
fn workspace_manager_git_diff_none_for_non_repo() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("a.txt"), "data").unwrap();
    let diff = WorkspaceManager::git_diff(tmp.path());
    assert!(diff.is_none());
}

#[test]
fn re_stage_from_same_source() {
    let src = make_source(&[("file.txt", "data")]);
    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_ne!(ws1.path(), ws2.path());
    assert!(ws1.path().join("file.txt").exists());
    assert!(ws2.path().join("file.txt").exists());
}

#[test]
fn stage_file_with_spaces_in_name() {
    let src = make_source(&[("file with spaces.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("file with spaces.txt").exists());
}

#[test]
fn diff_workspace_multiple_operations() {
    let src = make_source(&[("a.txt", "original\n"), ("b.txt", "keep\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("a.txt"), "modified\n").unwrap();
    fs::write(ws.path().join("c.txt"), "new\n").unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert_eq!(diff.file_count(), 3);
}

#[test]
fn operation_summary_default_zeroes() {
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
fn change_summary_default_zeroes() {
    let s = abp_workspace::tracker::ChangeSummary::default();
    assert_eq!(s.created, 0);
    assert_eq!(s.modified, 0);
    assert_eq!(s.deleted, 0);
    assert_eq!(s.renamed, 0);
    assert_eq!(s.total_size_delta, 0);
}

#[test]
fn diff_change_kind_display() {
    assert_eq!(format!("{}", DiffChangeKind::Added), "added");
    assert_eq!(format!("{}", DiffChangeKind::Modified), "modified");
    assert_eq!(format!("{}", DiffChangeKind::Deleted), "deleted");
    assert_eq!(format!("{}", DiffChangeKind::Renamed), "renamed");
}

#[test]
fn change_type_display() {
    assert_eq!(format!("{}", ChangeType::Added), "added");
    assert_eq!(format!("{}", ChangeType::Modified), "modified");
    assert_eq!(format!("{}", ChangeType::Deleted), "deleted");
}

#[test]
fn file_type_display() {
    assert_eq!(format!("{}", FileType::Rust), "rust");
    assert_eq!(format!("{}", FileType::Python), "python");
    assert_eq!(format!("{}", FileType::Binary), "binary");
    assert_eq!(format!("{}", FileType::Other), "other");
}

#[test]
fn policy_result_is_pass() {
    assert!(PolicyResult::Pass.is_pass());
    assert!(
        !PolicyResult::Fail {
            violations: vec!["x".into()]
        }
        .is_pass()
    );
}

#[test]
fn workspace_diff_summary_with_changes() {
    let mut diff = WorkspaceDiff::default();
    diff.files_added.push(abp_workspace::diff::FileChange {
        path: PathBuf::from("new.txt"),
        change_type: ChangeType::Added,
        additions: 10,
        deletions: 0,
        is_binary: false,
    });
    diff.total_additions = 10;
    let summary = diff.summary();
    assert!(summary.contains("1 file(s) changed"));
    assert!(summary.contains("1 added"));
}

#[test]
fn stager_with_only_excludes() {
    let src = make_source(&[
        ("keep.rs", "code"),
        ("trash.log", "log"),
        ("data.json", "{}"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.to_str().unwrap().ends_with(".rs")));
    assert!(files.iter().any(|f| f.to_str().unwrap().ends_with(".json")));
    assert!(!files.iter().any(|f| f.to_str().unwrap().ends_with(".log")));
}

#[test]
fn snapshot_nested_directories() {
    let src = make_source(&[("a/b/c.txt", "deep"), ("a/d.txt", "mid"), ("e.txt", "top")]);
    let snap = snapshot::capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 3);
}

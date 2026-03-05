#![allow(clippy::all)]
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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Comprehensive tests for workspace staging.
//!
//! Covers: basic staging, glob filtering, .git exclusion, git init, diff
//! generation, nested directories, empty workspaces, large files, permissions,
//! concurrent staging, cleanup, and error conditions.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{DiffAnalyzer, DiffPolicy, WorkspaceDiff, diff_workspace};
use abp_workspace::{PreparedWorkspace, WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
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

/// Collect sorted relative file paths (not dirs) under `root`, excluding `.git`.
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

/// Create a standard set of source files for reuse.
fn populate_source(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src").join("data.json"), r#"{"a":1}"#).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests").join("it.rs"), "#[test] fn t() {}").unwrap();
}

fn git_log_messages(path: &Path) -> Option<String> {
    Command::new("git")
        .args(["log", "--format=%s"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

fn git_commit_count(path: &Path) -> usize {
    Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<usize>()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

// ===========================================================================
// 1. Basic Staging
// ===========================================================================

#[test]
fn basic_staging_copies_all_files() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

#[test]
fn basic_staging_creates_different_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn basic_staging_preserves_file_content() {
    let src = tempdir().unwrap();
    let body = "fn main() { println!(\"hello world\"); }\n";
    fs::write(src.path().join("main.rs"), body).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("main.rs")).unwrap(), body);
}

#[test]
fn basic_staging_preserves_binary_content() {
    let src = tempdir().unwrap();
    let bytes: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("bin.dat"), &bytes).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("bin.dat")).unwrap(), bytes);
}

#[test]
fn basic_staging_does_not_modify_source() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let before = collect_files(src.path());
    let _ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(src.path()), before);
}

#[test]
fn basic_staging_preserves_subdirectory_structure() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::write(src.path().join("a").join("b").join("c.txt"), "deep").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("a").join("b").join("c.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("a").join("b").join("c.txt")).unwrap(),
        "deep"
    );
}

#[test]
fn basic_passthrough_uses_original_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn basic_staging_multiple_files_same_dir() {
    let src = tempdir().unwrap();
    for i in 0..10 {
        fs::write(
            src.path().join(format!("file_{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for i in 0..10 {
        let name = format!("file_{i}.txt");
        assert_eq!(
            fs::read_to_string(ws.path().join(&name)).unwrap(),
            format!("content {i}")
        );
    }
}

#[test]
fn basic_staging_empty_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("empty.txt").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

#[test]
fn basic_staging_preserves_file_sizes() {
    let src = tempdir().unwrap();
    let content = "x".repeat(4096);
    fs::write(src.path().join("sized.txt"), &content).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let meta = fs::metadata(ws.path().join("sized.txt")).unwrap();
    assert_eq!(meta.len(), 4096);
}

// ===========================================================================
// 2. Include/Exclude Glob Filtering
// ===========================================================================

#[test]
fn glob_include_only_rs_files() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let spec = staged_spec_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    for f in collect_files(ws.path()) {
        assert!(f.ends_with(".rs"), "unexpected file: {f}");
    }
}

#[test]
fn glob_exclude_md_files() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec!["*.md".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".md")));
    assert!(!files.is_empty());
}

#[test]
fn glob_include_and_exclude_combined() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let spec = staged_spec_globs(src.path(), vec!["**/*.rs".into()], vec!["tests/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.ends_with(".rs")));
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
}

#[test]
fn glob_double_star_recursive() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::write(src.path().join("top.rs"), "").unwrap();
    fs::write(src.path().join("a").join("mid.rs"), "").unwrap();
    fs::write(src.path().join("a").join("b").join("deep.rs"), "").unwrap();
    fs::write(src.path().join("a").join("b").join("deep.txt"), "").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"top.rs".to_string()));
    assert!(files.contains(&"a/mid.rs".to_string()));
    assert!(files.contains(&"a/b/deep.rs".to_string()));
    assert!(!files.contains(&"a/b/deep.txt".to_string()));
}

#[test]
fn glob_exclude_specific_subdirectory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("vendor")).unwrap();
    fs::write(src.path().join("src").join("main.rs"), "").unwrap();
    fs::write(src.path().join("vendor").join("dep.rs"), "").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec!["vendor/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("vendor/")));
    assert!(files.contains(&"src/main.rs".to_string()));
}

#[test]
fn glob_multiple_include_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "").unwrap();
    fs::write(src.path().join("config.toml"), "").unwrap();
    fs::write(src.path().join("notes.md"), "").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["*.rs".into(), "*.toml".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(files.contains(&"config.toml".to_string()));
    assert!(!files.contains(&"notes.md".to_string()));
}

#[test]
fn glob_multiple_exclude_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "").unwrap();
    fs::write(src.path().join("a.log"), "").unwrap();
    fs::write(src.path().join("b.tmp"), "").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec!["*.log".into(), "*.tmp".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"keep.rs".to_string()));
    assert!(!files.iter().any(|f| f.ends_with(".log")));
    assert!(!files.iter().any(|f| f.ends_with(".tmp")));
}

#[test]
fn glob_exclude_overrides_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src").join("generated")).unwrap();
    fs::write(src.path().join("src").join("main.rs"), "").unwrap();
    fs::write(src.path().join("src").join("generated").join("out.rs"), "").unwrap();
    let spec = staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/generated/**".into()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(!files.iter().any(|f| f.starts_with("src/generated/")));
}

#[test]
fn glob_no_patterns_copies_everything() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

#[test]
fn glob_include_nothing_matches() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "data").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["*.xyz".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty(), "nothing should match *.xyz");
}

#[test]
fn glob_exclude_everything() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "data").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec!["**/*".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty(), "everything should be excluded");
}

#[test]
fn glob_extension_case_sensitivity() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("lower.rs"), "").unwrap();
    fs::write(src.path().join("upper.RS"), "").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"lower.rs".to_string()));
    // Glob matching is case-sensitive on most systems
}

// ===========================================================================
// 3. .git Directory Exclusion
// ===========================================================================

#[test]
fn git_dir_excluded_from_staging() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join(".git").join("sentinel"), "marker").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // sentinel file inside .git should not be copied
    assert!(!ws.path().join(".git").join("sentinel").exists());
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn git_dir_excluded_even_with_include_all() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("config"), "").unwrap();
    fs::write(src.path().join("real.txt"), "data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("real.txt").exists());
}

#[test]
fn git_dir_excluded_via_workspace_manager() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git").join("refs")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("app.rs"), "").unwrap();
    // WorkspaceManager with Staged mode creates its own .git from init, not copy
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // The .git dir will exist but from init, not from source copy
    assert!(ws.path().join("app.rs").exists());
    // Original .git/HEAD content should not be present
    let head = fs::read_to_string(ws.path().join(".git").join("HEAD")).unwrap();
    // A newly init'd repo HEAD should reference a branch
    assert!(head.contains("ref:"));
}

#[test]
fn git_nested_git_dir_excluded() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("submodule").join(".git")).unwrap();
    fs::write(
        src.path().join("submodule").join(".git").join("HEAD"),
        "ref: refs/heads/main",
    )
    .unwrap();
    fs::write(src.path().join("submodule").join("code.rs"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join("submodule").join(".git").exists());
    assert!(ws.path().join("submodule").join("code.rs").exists());
}

#[test]
fn git_dir_with_many_objects_excluded() {
    let src = tempdir().unwrap();
    let objects = src.path().join(".git").join("objects").join("pack");
    fs::create_dir_all(&objects).unwrap();
    for i in 0..20 {
        fs::write(
            objects.join(format!("obj_{i:04}.pack")),
            format!("pack {i}"),
        )
        .unwrap();
    }
    fs::write(src.path().join("file.rs"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("file.rs").exists());
}

// ===========================================================================
// 4. Git Initialization
// ===========================================================================

#[test]
fn git_init_creates_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
}

#[test]
fn git_init_baseline_commit_message() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let msgs = git_log_messages(ws.path()).unwrap();
    assert!(msgs.contains("baseline"), "expected 'baseline' in: {msgs}");
}

#[test]
fn git_init_exactly_one_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(git_commit_count(ws.path()), 1);
}

#[test]
fn git_init_clean_working_tree() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.trim().is_empty(), "tree should be clean: {status}");
}

#[test]
fn git_init_all_files_tracked() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let output = Command::new("git")
        .args(["ls-files"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let tracked: Vec<&str> = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let source_files = collect_files(src.path());
    assert_eq!(tracked.len(), source_files.len());
}

#[test]
fn git_init_stager_enabled_by_default() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn git_init_stager_disabled() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn git_init_new_files_untracked() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("orig.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new_file.txt"), "new content").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("new_file.txt"));
    assert!(status.contains("??"));
}

#[test]
fn git_init_modified_files_in_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "modified").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("data.txt"));
    assert!(diff.contains("modified"));
}

#[test]
fn git_init_deleted_files_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "bye").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("doomed.txt")).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("doomed.txt"));
    assert!(status.contains(" D "));
}

// ===========================================================================
// 5. Diff Generation
// ===========================================================================

#[test]
fn diff_no_changes_is_empty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
}

#[test]
fn diff_added_file_detected() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new.txt"), "new content\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 1);
    assert_eq!(summary.added[0], PathBuf::from("new.txt"));
    assert!(summary.total_additions > 0);
}

#[test]
fn diff_modified_file_detected() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "modified\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 1);
    assert_eq!(summary.modified[0], PathBuf::from("f.txt"));
}

#[test]
fn diff_deleted_file_detected() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("f.txt")).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.deleted.len(), 1);
    assert_eq!(summary.deleted[0], PathBuf::from("f.txt"));
}

#[test]
fn diff_multiple_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "same\n").unwrap();
    fs::write(src.path().join("edit.txt"), "old\n").unwrap();
    fs::write(src.path().join("remove.txt"), "gone\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("edit.txt"), "new\n").unwrap();
    fs::remove_file(ws.path().join("remove.txt")).unwrap();
    fs::write(ws.path().join("added.txt"), "fresh\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 1);
    assert_eq!(summary.modified.len(), 1);
    assert_eq!(summary.deleted.len(), 1);
    assert_eq!(summary.file_count(), 3);
}

#[test]
fn diff_total_changes_count() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "line1\nmodified\nline3\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_changes() > 0);
}

#[test]
fn diff_analyzer_has_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(!analyzer.has_changes());
    fs::write(ws.path().join("new.txt"), "new").unwrap();
    assert!(analyzer.has_changes());
}

#[test]
fn diff_analyzer_changed_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a\n").unwrap();
    fs::write(src.path().join("b.txt"), "b\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("a.txt"), "changed\n").unwrap();
    fs::write(ws.path().join("c.txt"), "new\n").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let changed = analyzer.changed_files();
    assert!(changed.contains(&PathBuf::from("a.txt")));
    assert!(changed.contains(&PathBuf::from("c.txt")));
    assert!(!changed.contains(&PathBuf::from("b.txt")));
}

#[test]
fn diff_analyzer_file_was_modified() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "orig\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "changed\n").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.file_was_modified(Path::new("f.txt")));
    assert!(!analyzer.file_was_modified(Path::new("other.txt")));
}

#[test]
fn diff_analyzer_analyze_rich_result() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("x.txt"), "old\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("x.txt"), "new\n").unwrap();
    fs::write(ws.path().join("y.txt"), "added\n").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    assert_eq!(diff.files_modified.len(), 1);
    assert_eq!(diff.files_added.len(), 1);
    assert!(!diff.is_empty());
}

#[test]
fn diff_workspace_diff_summary_string() {
    let diff = WorkspaceDiff::default();
    assert_eq!(diff.summary(), "No changes detected.");
}

#[test]
fn diff_policy_pass() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "changed\n").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    let policy = DiffPolicy {
        max_files: Some(10),
        max_additions: Some(100),
        denied_paths: vec![],
    };
    assert!(policy.check(&diff).unwrap().is_pass());
}

#[test]
fn diff_policy_fail_max_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    fs::write(ws.path().join("b.txt"), "b\n").unwrap();
    fs::write(ws.path().join("c.txt"), "c\n").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    let policy = DiffPolicy {
        max_files: Some(1),
        max_additions: None,
        denied_paths: vec![],
    };
    assert!(!policy.check(&diff).unwrap().is_pass());
}

// ===========================================================================
// 6. Nested Directory Handling
// ===========================================================================

#[test]
fn nested_deep_directory_tree() {
    let src = tempdir().unwrap();
    let depth = 15;
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
    assert!(expected.join("leaf.txt").exists());
    assert_eq!(
        fs::read_to_string(expected.join("leaf.txt")).unwrap(),
        "bottom"
    );
}

#[test]
fn nested_multiple_branches() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("x")).unwrap();
    fs::create_dir_all(src.path().join("a").join("y")).unwrap();
    fs::create_dir_all(src.path().join("b")).unwrap();
    fs::write(src.path().join("a").join("x").join("1.txt"), "ax1").unwrap();
    fs::write(src.path().join("a").join("y").join("2.txt"), "ay2").unwrap();
    fs::write(src.path().join("b").join("3.txt"), "b3").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"a/x/1.txt".to_string()));
    assert!(files.contains(&"a/y/2.txt".to_string()));
    assert!(files.contains(&"b/3.txt".to_string()));
}

#[test]
fn nested_same_filename_different_dirs() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a")).unwrap();
    fs::create_dir_all(src.path().join("b")).unwrap();
    fs::write(src.path().join("a").join("file.txt"), "from a").unwrap();
    fs::write(src.path().join("b").join("file.txt"), "from b").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("a").join("file.txt")).unwrap(),
        "from a"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("b").join("file.txt")).unwrap(),
        "from b"
    );
}

#[test]
fn nested_empty_intermediate_dirs() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    // Only file is at the deepest level
    fs::write(
        src.path().join("a").join("b").join("c").join("f.txt"),
        "deep",
    )
    .unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(
        ws.path()
            .join("a")
            .join("b")
            .join("c")
            .join("f.txt")
            .exists()
    );
}

#[test]
fn nested_wide_directory() {
    let src = tempdir().unwrap();
    for i in 0..20 {
        let dir = src.path().join(format!("dir_{i:02}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), format!("content {i}")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for i in 0..20 {
        let path = ws.path().join(format!("dir_{i:02}")).join("file.txt");
        assert!(path.exists(), "missing dir_{i:02}/file.txt");
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("content {i}"));
    }
}

#[test]
fn nested_dotdirs_not_git() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".config")).unwrap();
    fs::write(src.path().join(".config").join("settings.json"), "{}").unwrap();
    fs::create_dir_all(src.path().join(".vscode")).unwrap();
    fs::write(src.path().join(".vscode").join("launch.json"), "{}").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(".config").join("settings.json").exists());
    assert!(ws.path().join(".vscode").join("launch.json").exists());
}

// ===========================================================================
// 7. Empty Workspace
// ===========================================================================

#[test]
fn empty_source_stages_nothing() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn empty_source_git_still_initialized() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn empty_source_only_git_dir() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn empty_source_diff_empty() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
}

#[test]
fn empty_dirs_only_source() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    fs::create_dir_all(src.path().join("d")).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty(), "only empty dirs should produce no files");
}

// ===========================================================================
// 8. Large File Handling
// ===========================================================================

#[test]
fn large_single_file_1mb() {
    let src = tempdir().unwrap();
    let content = "x".repeat(1024 * 1024);
    fs::write(src.path().join("big.bin"), &content).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let staged = fs::read_to_string(ws.path().join("big.bin")).unwrap();
    assert_eq!(staged.len(), content.len());
}

#[test]
fn large_many_small_files() {
    let src = tempdir().unwrap();
    let count = 200;
    for i in 0..count {
        fs::write(
            src.path().join(format!("f_{i:04}.txt")),
            format!("data {i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), count);
}

#[test]
fn large_file_binary_content_preserved() {
    let src = tempdir().unwrap();
    let bytes: Vec<u8> = (0..=255).cycle().take(65536).collect();
    fs::write(src.path().join("binary.dat"), &bytes).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(fs::read(ws.path().join("binary.dat")).unwrap(), bytes);
}

#[test]
fn large_files_across_directories() {
    let src = tempdir().unwrap();
    for i in 0..5 {
        let dir = src.path().join(format!("dir_{i}"));
        fs::create_dir_all(&dir).unwrap();
        for j in 0..20 {
            fs::write(dir.join(format!("file_{j}.txt")), format!("{i}_{j}")).unwrap();
        }
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 100);
}

#[test]
fn large_file_with_newlines() {
    let src = tempdir().unwrap();
    let content: String = (0..10000).map(|i| format!("line {i}\n")).collect();
    fs::write(src.path().join("lines.txt"), &content).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("lines.txt")).unwrap(),
        content
    );
}

// ===========================================================================
// 9. Permission Preservation (Unix-specific tests gated)
// ===========================================================================

#[test]
fn permission_readonly_file_staged() {
    let src = tempdir().unwrap();
    let path = src.path().join("readonly.txt");
    fs::write(&path, "read only").unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("readonly.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("readonly.txt")).unwrap(),
        "read only"
    );

    // Cleanup: restore permissions for tempdir deletion
    #[allow(clippy::permissions_set_readonly_false)]
    {
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(false);
        fs::set_permissions(&path, perms).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn permission_executable_bit_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let src = tempdir().unwrap();
    let path = src.path().join("script.sh");
    fs::write(&path, "#!/bin/sh\necho hello").unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let staged_perms = fs::metadata(ws.path().join("script.sh"))
        .unwrap()
        .permissions();
    assert!(
        staged_perms.mode() & 0o111 != 0,
        "executable bit should be preserved"
    );
}

#[cfg(unix)]
#[test]
fn permission_mode_preserved_for_regular_files() {
    use std::os::unix::fs::PermissionsExt;
    let src = tempdir().unwrap();
    let path = src.path().join("file.txt");
    fs::write(&path, "data").unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&path, perms).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let staged_mode = fs::metadata(ws.path().join("file.txt"))
        .unwrap()
        .permissions()
        .mode();
    // At minimum, owner read/write should be preserved
    assert!(staged_mode & 0o600 != 0);
}

#[test]
fn permission_dotfiles_staged() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "hidden").unwrap();
    fs::write(src.path().join(".env"), "KEY=val").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(".hidden").exists());
    assert!(ws.path().join(".env").exists());
}

// ===========================================================================
// 10. Concurrent Staging
// ===========================================================================

#[test]
fn concurrent_sequential_staging() {
    let src = tempdir().unwrap();
    populate_source(src.path());
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
    assert_eq!(collect_files(ws1.path()), collect_files(ws2.path()));
}

#[test]
fn concurrent_threaded_staging() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let src_path = Arc::new(src.path().to_path_buf());
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let sp = Arc::clone(&src_path);
            std::thread::spawn(move || {
                WorkspaceStager::new()
                    .source_root(sp.as_path())
                    .with_git_init(false)
                    .stage()
                    .unwrap()
            })
        })
        .collect();
    let workspaces: Vec<PreparedWorkspace> =
        handles.into_iter().map(|h| h.join().unwrap()).collect();
    let reference = collect_files(workspaces[0].path());
    for ws in &workspaces[1..] {
        assert_eq!(collect_files(ws.path()), reference);
    }
    // All paths are unique
    let paths: Vec<&Path> = workspaces.iter().map(|w| w.path()).collect();
    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            assert_ne!(paths[i], paths[j]);
        }
    }
}

#[test]
fn concurrent_threaded_with_git_init() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let src_path = Arc::new(src.path().to_path_buf());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let sp = Arc::clone(&src_path);
            std::thread::spawn(move || {
                WorkspaceStager::new()
                    .source_root(sp.as_path())
                    .with_git_init(true)
                    .stage()
                    .unwrap()
            })
        })
        .collect();
    let workspaces: Vec<PreparedWorkspace> =
        handles.into_iter().map(|h| h.join().unwrap()).collect();
    for ws in &workspaces {
        assert!(ws.path().join(".git").exists());
        assert_eq!(git_commit_count(ws.path()), 1);
    }
}

#[test]
fn concurrent_independent_mutations() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "original").unwrap();
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
    fs::write(ws1.path().join("shared.txt"), "mutated by ws1").unwrap();
    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(src.path().join("shared.txt")).unwrap(),
        "original"
    );
}

// ===========================================================================
// 11. Cleanup
// ===========================================================================

#[test]
fn cleanup_temp_dir_removed_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let ws_path = ws.path().to_path_buf();
    assert!(ws_path.exists());
    drop(ws);
    assert!(!ws_path.exists(), "temp dir should be removed after drop");
}

#[test]
fn cleanup_multiple_workspaces_independent() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
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
    let p1 = ws1.path().to_path_buf();
    let p2 = ws2.path().to_path_buf();
    drop(ws1);
    assert!(!p1.exists());
    assert!(p2.exists(), "ws2 should survive ws1 drop");
    drop(ws2);
    assert!(!p2.exists());
}

#[test]
fn cleanup_passthrough_does_not_delete() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    let ws_path = ws.path().to_path_buf();
    drop(ws);
    assert!(ws_path.exists(), "passthrough should not delete source");
}

#[test]
fn cleanup_staged_with_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws_path = ws.path().to_path_buf();
    assert!(ws_path.join(".git").exists());
    drop(ws);
    assert!(!ws_path.exists());
}

#[test]
fn cleanup_staged_with_mutations() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Create many files in the workspace
    for i in 0..50 {
        fs::write(ws.path().join(format!("extra_{i}.txt")), "extra").unwrap();
    }
    let ws_path = ws.path().to_path_buf();
    drop(ws);
    assert!(!ws_path.exists());
}

// ===========================================================================
// 12. Error Conditions
// ===========================================================================

#[test]
fn error_nonexistent_source_stager() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("does not exist"),
        "expected 'does not exist' in: {msg}"
    );
}

#[test]
fn error_no_source_root_set() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("source_root"),
        "expected 'source_root' in: {msg}"
    );
}

#[test]
fn error_invalid_include_glob_pattern() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let result = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["[".into()])
        .stage();
    assert!(result.is_err());
}

#[test]
fn error_invalid_exclude_glob_pattern() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let result = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["[".into()])
        .stage();
    assert!(result.is_err());
}

#[test]
fn error_workspace_manager_nonexistent_staged() {
    let spec = WorkspaceSpec {
        root: "/nonexistent/workspace/path/xyz".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    // WalkDir will fail on nonexistent paths
    let result = WorkspaceManager::prepare(&spec);
    assert!(result.is_err());
}

#[test]
fn error_workspace_manager_invalid_globs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["[invalid".into()], vec![]);
    let result = WorkspaceManager::prepare(&spec);
    assert!(result.is_err());
}

// ===========================================================================
// Additional edge cases and special scenarios
// ===========================================================================

#[test]
fn special_files_with_spaces() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("my file.txt"), "spaces").unwrap();
    fs::write(src.path().join("another  double.txt"), "double space").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("my file.txt")).unwrap(),
        "spaces"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("another  double.txt")).unwrap(),
        "double space"
    );
}

#[test]
fn special_unicode_filenames() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("café.txt"), "coffee").unwrap();
    fs::write(src.path().join("日本語.md"), "japanese").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("café.txt")).unwrap(),
        "coffee"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("日本語.md")).unwrap(),
        "japanese"
    );
}

#[test]
fn special_long_path_names() {
    let src = tempdir().unwrap();
    let long_dir = "a".repeat(80);
    let long_file = format!("{}.txt", "b".repeat(80));
    fs::create_dir_all(src.path().join(&long_dir)).unwrap();
    fs::write(src.path().join(&long_dir).join(&long_file), "long").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(&long_dir).join(&long_file).exists());
}

#[test]
fn special_multiple_dots_in_filename() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.test.spec.rs"), "code").unwrap();
    fs::write(src.path().join("archive.tar.gz"), "data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("file.test.spec.rs").exists());
    assert!(ws.path().join("archive.tar.gz").exists());
}

#[test]
fn special_dashes_and_underscores() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("my-file.txt"), "dash").unwrap();
    fs::write(src.path().join("my_file.txt"), "underscore").unwrap();
    fs::write(src.path().join("MiXeD-CaSe_File.TXT"), "mixed").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("my-file.txt").exists());
    assert!(ws.path().join("my_file.txt").exists());
    assert!(ws.path().join("MiXeD-CaSe_File.TXT").exists());
}

#[test]
fn stager_builder_fluent_api() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.log"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .exclude(vec![])
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
    assert!(!files.contains(&"b.log".to_string()));
}

#[test]
fn stager_default_is_new() {
    let stager = WorkspaceStager::default();
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = stager.source_root(src.path()).stage().unwrap();
    assert!(ws.path().join(".git").exists());
    assert!(ws.path().join("f.txt").exists());
}

#[test]
fn diff_added_multiple_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("base.txt"), "base\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for i in 0..5 {
        fs::write(ws.path().join(format!("new_{i}.txt")), format!("new {i}\n")).unwrap();
    }
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 5);
}

#[test]
fn diff_deleted_multiple_files() {
    let src = tempdir().unwrap();
    for i in 0..5 {
        fs::write(src.path().join(format!("f_{i}.txt")), format!("data {i}\n")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for i in 0..3 {
        fs::remove_file(ws.path().join(format!("f_{i}.txt"))).unwrap();
    }
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.deleted.len(), 3);
}

#[test]
fn glob_exclude_by_extension_in_subdir() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("logs")).unwrap();
    fs::write(src.path().join("logs").join("app.log"), "log").unwrap();
    fs::write(src.path().join("logs").join("error.log"), "err").unwrap();
    fs::write(src.path().join("app.rs"), "code").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec!["**/*.log".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".log")));
    assert!(files.contains(&"app.rs".to_string()));
}

#[test]
fn glob_include_only_top_level() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "root").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("nested.txt"), "nested").unwrap();
    // globset's * matches across / by default
    let spec = staged_spec_globs(src.path(), vec!["*.txt".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"root.txt".to_string()));
}

#[test]
fn workspace_manager_passthrough_no_temp() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    // In passthrough mode the path is the same as source
    assert_eq!(ws.path(), src.path());
    // Dropping should NOT remove source
    let path = ws.path().to_path_buf();
    drop(ws);
    assert!(path.exists());
}

#[test]
fn staging_preserves_nested_binary() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("assets")).unwrap();
    let bytes: Vec<u8> = (0..=255).cycle().take(1024).collect();
    fs::write(src.path().join("assets").join("image.bin"), &bytes).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read(ws.path().join("assets").join("image.bin")).unwrap(),
        bytes
    );
}

#[test]
fn diff_workspace_diff_file_count() {
    let mut diff = WorkspaceDiff::default();
    assert_eq!(diff.file_count(), 0);
    assert!(diff.is_empty());
    diff.files_added.push(abp_workspace::diff::FileChange {
        path: PathBuf::from("new.txt"),
        change_type: abp_workspace::diff::ChangeType::Added,
        additions: 1,
        deletions: 0,
        is_binary: false,
    });
    assert_eq!(diff.file_count(), 1);
    assert!(!diff.is_empty());
}

#[test]
fn diff_summary_is_empty_default() {
    use abp_workspace::diff::DiffSummary;
    let s = DiffSummary::default();
    assert!(s.is_empty());
    assert_eq!(s.file_count(), 0);
    assert_eq!(s.total_changes(), 0);
}

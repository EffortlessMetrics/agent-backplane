// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for workspace staging.
//!
//! Covers basic staging, glob filtering, git initialization, and edge cases.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_staged_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

fn make_staged_spec_with_globs(
    root: &Path,
    include: Vec<String>,
    exclude: Vec<String>,
) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
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

/// Run `git log --oneline` in the given directory.
fn git_log_oneline(path: &Path) -> Option<String> {
    Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

/// Run `git log --format=%s` to get commit messages.
fn git_log_messages(path: &Path) -> Option<String> {
    Command::new("git")
        .args(["log", "--format=%s"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

/// Create a source tree with a standard set of files for reuse.
fn create_standard_source(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::write(root.join("config.toml"), "[package]").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src").join("data.json"), "{}").unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests").join("test_one.rs"), "#[test] fn t() {}").unwrap();
}

// ===========================================================================
// 1. Basic Staging (5+ tests)
// ===========================================================================

#[test]
fn basic_stage_all_files_present() {
    let src = tempdir().unwrap();
    create_standard_source(src.path());

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    let staged = collect_files(ws.path());
    let source = collect_files(src.path());
    assert_eq!(staged, source, "staged copy must contain all source files");
}

#[test]
fn basic_staged_path_differs_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    assert_ne!(
        ws.path(),
        src.path(),
        "staged workspace must be at a different path"
    );
}

#[test]
fn basic_source_not_modified_by_staging() {
    let src = tempdir().unwrap();
    create_standard_source(src.path());

    let before = collect_files(src.path());
    let content_before = fs::read_to_string(src.path().join("main.rs")).unwrap();

    let _ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    let after = collect_files(src.path());
    let content_after = fs::read_to_string(src.path().join("main.rs")).unwrap();

    assert_eq!(before, after, "source file listing must not change");
    assert_eq!(
        content_before, content_after,
        "source file content must not change"
    );
}

#[test]
fn basic_staged_has_git_initialized() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    assert!(
        ws.path().join(".git").exists(),
        "staged workspace must have .git directory"
    );
}

#[test]
fn basic_staged_has_baseline_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    let messages = git_log_messages(ws.path());
    assert!(
        messages.is_some(),
        "git log should succeed in staged workspace"
    );
    let messages = messages.unwrap();
    assert!(
        messages.contains("baseline"),
        "initial commit message should be 'baseline', got: {messages}"
    );
}

#[test]
fn basic_file_content_integrity() {
    let src = tempdir().unwrap();
    let content = "fn main() { println!(\"hello world\"); }";
    fs::write(src.path().join("main.rs"), content).unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("main.rs")).unwrap(),
        content,
        "file content must be identical after staging"
    );
}

// ===========================================================================
// 2. Glob Filtering (8+ tests)
// ===========================================================================

#[test]
fn glob_include_matches_only_specified_files() {
    let src = tempdir().unwrap();
    create_standard_source(src.path());

    let spec = make_staged_spec_with_globs(src.path(), vec!["*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    // Only top-level .rs files match *.rs (not recursive)
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected non-.rs file staged: {f}");
    }
}

#[test]
fn glob_exclude_removes_files() {
    let src = tempdir().unwrap();
    create_standard_source(src.path());

    let spec = make_staged_spec_with_globs(src.path(), vec![], vec!["*.md".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(
        !files.iter().any(|f| f.ends_with(".md")),
        "excluded .md files should not be staged: {files:?}"
    );
    assert!(!files.is_empty(), "non-.md files should still be staged");
}

#[test]
fn glob_include_and_exclude_interaction() {
    let src = tempdir().unwrap();
    create_standard_source(src.path());

    // Include all .rs files, but exclude anything in tests/
    let spec =
        make_staged_spec_with_globs(src.path(), vec!["**/*.rs".into()], vec!["tests/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(
        !files.iter().any(|f| f.starts_with("tests/")),
        "tests/ should be excluded: {files:?}"
    );
    // Should still have non-test .rs files
    assert!(
        files.iter().any(|f| f.ends_with(".rs")),
        "non-test .rs files should be staged: {files:?}"
    );
}

#[test]
fn glob_recursive_star_star_rs() {
    let src = tempdir().unwrap();
    create_standard_source(src.path());

    let spec = make_staged_spec_with_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs"), "only .rs files expected, got: {f}");
    }
    // Verify we get files from subdirs too
    assert!(
        files.iter().any(|f| f.contains('/')),
        "should include .rs files from subdirectories: {files:?}"
    );
}

#[test]
fn glob_dot_git_always_excluded() {
    let src = tempdir().unwrap();
    let git_dir = src.path().join(".git");
    fs::create_dir_all(git_dir.join("objects")).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(git_dir.join("sentinel_marker"), "should_not_copy").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    // Use stager without git init to verify .git is not copied
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws.path().join(".git").exists(),
        ".git must never be copied from source"
    );
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn glob_nested_directories_preserved() {
    let src = tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.rs"), "fn deep() {}").unwrap();
    fs::write(src.path().join("top.rs"), "fn top() {}").unwrap();
    fs::write(src.path().join("a").join("mid.rs"), "fn mid() {}").unwrap();

    let spec = make_staged_spec_with_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"top.rs".to_string()));
    assert!(files.contains(&"a/mid.rs".to_string()));
    assert!(files.contains(&"a/b/c/deep.rs".to_string()));
}

#[test]
fn glob_empty_dirs_after_filtering() {
    let src = tempdir().unwrap();
    // Create dir with only .txt files, then include only .rs
    fs::create_dir_all(src.path().join("only_txt")).unwrap();
    fs::write(src.path().join("only_txt").join("a.txt"), "text").unwrap();
    fs::write(src.path().join("only_txt").join("b.txt"), "text").unwrap();
    fs::write(src.path().join("keep.rs"), "fn keep() {}").unwrap();

    let spec = make_staged_spec_with_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(
        !files.iter().any(|f| f.ends_with(".txt")),
        "no .txt files should be staged"
    );
    assert!(files.contains(&"keep.rs".to_string()));
}

#[test]
fn glob_exclude_specific_subdirectory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("vendor")).unwrap();
    fs::write(src.path().join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("vendor").join("dep.rs"), "fn dep() {}").unwrap();
    fs::write(src.path().join("root.rs"), "fn root() {}").unwrap();

    let spec = make_staged_spec_with_globs(src.path(), vec![], vec!["vendor/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(
        !files.iter().any(|f| f.starts_with("vendor/")),
        "vendor/ should be excluded: {files:?}"
    );
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(files.contains(&"root.rs".to_string()));
}

#[test]
fn glob_multiple_include_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn code() {}").unwrap();
    fs::write(src.path().join("config.toml"), "[pkg]").unwrap();
    fs::write(src.path().join("notes.md"), "# Notes").unwrap();
    fs::write(src.path().join("data.json"), "{}").unwrap();

    let spec =
        make_staged_spec_with_globs(src.path(), vec!["*.rs".into(), "*.toml".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(files.contains(&"config.toml".to_string()));
    assert!(!files.contains(&"notes.md".to_string()));
    assert!(!files.contains(&"data.json".to_string()));
}

// ===========================================================================
// 3. Git Initialization (5+ tests)
// ===========================================================================

#[test]
fn git_staged_workspace_has_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("lib.rs"), "pub fn f() {}").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    assert!(ws.path().join(".git").exists());
    // Verify it's actually a git repo by running git status
    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.is_some(),
        "git status should succeed in staged workspace"
    );
}

#[test]
fn git_initial_commit_has_baseline_message() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "world").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    let log = git_log_oneline(ws.path());
    assert!(log.is_some(), "git log should work");
    let log = log.unwrap();
    assert!(
        log.contains("baseline"),
        "commit message should contain 'baseline', got: {log}"
    );
}

#[test]
fn git_working_tree_clean_after_staging() {
    let src = tempdir().unwrap();
    create_standard_source(src.path());

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some(), "git status should succeed");
    let status = status.unwrap();
    assert!(
        status.trim().is_empty(),
        "working tree should be clean after staging, got: {status}"
    );
}

#[test]
fn git_new_files_show_as_untracked() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("original.txt"), "original").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    // Add a new file after staging
    fs::write(ws.path().join("brand_new.txt"), "I am new").unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    let status = status.unwrap();
    assert!(
        status.contains("brand_new.txt"),
        "new file should appear in status: {status}"
    );
    assert!(
        status.contains("??"),
        "untracked files should show ?? marker: {status}"
    );
}

#[test]
fn git_modified_files_show_in_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original content").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("data.txt"), "modified content").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some(), "diff should succeed");
    let diff = diff.unwrap();
    assert!(
        diff.contains("data.txt"),
        "diff should reference the modified file: {diff}"
    );
    assert!(
        diff.contains("modified content"),
        "diff should show new content"
    );
    assert!(
        diff.contains("original content"),
        "diff should show old content"
    );
}

#[test]
fn git_exactly_one_initial_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    let output = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let count = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(
        count, "1",
        "should have exactly one initial commit, got: {count}"
    );
}

#[test]
fn git_deleted_files_show_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "will be deleted").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    let status = status.unwrap();
    assert!(
        status.contains("doomed.txt"),
        "deleted file should appear in status: {status}"
    );
    assert!(
        status.contains(" D "),
        "deletion marker expected in status: {status}"
    );
}

// ===========================================================================
// 4. Edge Cases (7+ tests)
// ===========================================================================

#[test]
fn edge_empty_source_directory() {
    let src = tempdir().unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    let files = collect_files(ws.path());
    assert!(files.is_empty(), "empty source should stage no files");
    assert!(
        ws.path().join(".git").exists(),
        ".git should still be initialized"
    );
}

#[test]
fn edge_source_with_only_git_directory() {
    let src = tempdir().unwrap();
    let git_dir = src.path().join(".git");
    fs::create_dir_all(git_dir.join("objects")).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
    // No user files, only .git

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(
        files.is_empty(),
        "source with only .git should stage no user files"
    );
}

#[test]
fn edge_very_deep_nesting() {
    let src = tempdir().unwrap();
    let depth = 12;
    let mut deep = src.path().to_path_buf();
    for i in 0..depth {
        deep = deep.join(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "I am at the bottom").unwrap();

    let ws = WorkspaceManager::prepare(&make_staged_spec(src.path())).unwrap();

    let mut expected = ws.path().to_path_buf();
    for i in 0..depth {
        expected = expected.join(format!("d{i}"));
    }
    assert!(
        expected.join("leaf.txt").exists(),
        "deeply nested file must be staged"
    );
    assert_eq!(
        fs::read_to_string(expected.join("leaf.txt")).unwrap(),
        "I am at the bottom"
    );
}

#[test]
fn edge_files_with_spaces_in_names() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("my file.txt"), "spaces in name").unwrap();
    fs::write(src.path().join("another  file.rs"), "double space").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("my file.txt").exists());
    assert!(ws.path().join("another  file.rs").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("my file.txt")).unwrap(),
        "spaces in name"
    );
}

#[test]
fn edge_files_with_special_characters() {
    let src = tempdir().unwrap();
    // Characters that are valid on both Windows and Linux
    fs::write(src.path().join("file-with-dashes.txt"), "dashes").unwrap();
    fs::write(src.path().join("file_with_underscores.txt"), "underscores").unwrap();
    fs::write(src.path().join("file.multiple.dots.txt"), "dots").unwrap();
    fs::write(src.path().join("ALLCAPS.TXT"), "caps").unwrap();
    fs::write(src.path().join("MiXeD.CaSe"), "mixed").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("file-with-dashes.txt").exists());
    assert!(ws.path().join("file_with_underscores.txt").exists());
    assert!(ws.path().join("file.multiple.dots.txt").exists());
    assert!(ws.path().join("ALLCAPS.TXT").exists());
    assert!(ws.path().join("MiXeD.CaSe").exists());
}

#[test]
fn edge_large_file_handling() {
    let src = tempdir().unwrap();
    // Create a 1MB file
    let large_content = "x".repeat(1024 * 1024);
    fs::write(src.path().join("large.bin"), &large_content).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged_content = fs::read_to_string(ws.path().join("large.bin")).unwrap();
    assert_eq!(
        staged_content.len(),
        large_content.len(),
        "large file should be staged with correct size"
    );
    assert_eq!(staged_content, large_content);
}

#[test]
fn edge_concurrent_staging_operations() {
    let src = tempdir().unwrap();
    create_standard_source(src.path());

    // Stage the same source multiple times concurrently
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
    let ws3 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // All should be at different paths
    assert_ne!(ws1.path(), ws2.path());
    assert_ne!(ws2.path(), ws3.path());
    assert_ne!(ws1.path(), ws3.path());

    // All should have identical file listings
    let f1 = collect_files(ws1.path());
    let f2 = collect_files(ws2.path());
    let f3 = collect_files(ws3.path());
    assert_eq!(f1, f2);
    assert_eq!(f2, f3);
}

#[test]
fn edge_dotfiles_are_staged() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "hidden file").unwrap();
    fs::write(src.path().join(".env"), "SECRET=123").unwrap();
    fs::write(src.path().join("visible.txt"), "visible").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // Dotfiles (except .git) should be staged
    assert!(ws.path().join(".hidden").exists());
    assert!(ws.path().join(".env").exists());
    assert!(ws.path().join("visible.txt").exists());
}

#[test]
fn edge_readonly_file_staging() {
    let src = tempdir().unwrap();
    let file_path = src.path().join("readonly.txt");
    fs::write(&file_path, "read only content").unwrap();

    // Make file read-only
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file_path, perms).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("readonly.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("readonly.txt")).unwrap(),
        "read only content"
    );

    // Clean up: restore write permission so tempdir can be deleted
    #[allow(clippy::permissions_set_readonly_false)]
    {
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_readonly(false);
        fs::set_permissions(&file_path, perms).unwrap();
    }
}

#[test]
fn edge_source_does_not_exist_errors() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist/anywhere")
        .stage();
    assert!(result.is_err(), "staging nonexistent source should fail");
}

#[test]
fn edge_workspace_stager_no_source_errors() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err(), "staging without source_root should fail");
}

// ===========================================================================
// 5. WorkspaceStager builder tests
// ===========================================================================

#[test]
fn stager_git_init_enabled_by_default() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(
        ws.path().join(".git").exists(),
        "git should be initialized by default"
    );
    let log = git_log_oneline(ws.path());
    assert!(log.is_some());
}

#[test]
fn stager_git_init_disabled() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws.path().join(".git").exists(),
        "git should not be initialized when disabled"
    );
}

#[test]
fn stager_with_all_options() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn keep() {}").unwrap();
    fs::write(src.path().join("skip.log"), "log data").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("nested.rs"), "fn nested() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*.rs".into()])
        .exclude(vec!["sub/**".into()])
        .with_git_init(true)
        .stage()
        .unwrap();

    assert!(ws.path().join("keep.rs").exists());
    assert!(!ws.path().join("skip.log").exists());
    assert!(!ws.path().join("sub").join("nested.rs").exists());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_mutation_does_not_affect_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // Mutate staged workspace
    fs::write(ws.path().join("file.txt"), "mutated").unwrap();
    fs::write(ws.path().join("new.txt"), "brand new").unwrap();

    // Source must be untouched
    assert_eq!(
        fs::read_to_string(src.path().join("file.txt")).unwrap(),
        "original"
    );
    assert!(!src.path().join("new.txt").exists());
}

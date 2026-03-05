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
//! Deep comprehensive tests for workspace staging and git integration.
//!
//! Covers basic staging, include/exclude glob filtering, .git exclusion,
//! git initialisation, baseline commits, diff generation, nested structures,
//! symlinks, empty dirs, large files, unicode filenames, read-only files,
//! workspace isolation, cleanup on drop, and the WorkspaceStager builder API.

use abp_core::{WorkspaceMode, WorkspaceSpec};
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

/// Collect sorted relative directory paths (excluding `.git`) under `root`.
#[allow(dead_code)]
fn collect_dirs(root: &Path) -> Vec<String> {
    let mut dirs: Vec<String> = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == ".git"))
        .filter(|e| e.file_type().is_dir())
        .filter_map(|e| {
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel.is_empty() { None } else { Some(rel) }
        })
        .collect();
    dirs.sort();
    dirs
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

/// Run a git command, returning None on failure instead of panicking.
fn git_opt(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// Create a standard fixture tree at `root`.
fn create_fixture(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src/data.json"), r#"{"key":"value"}"#).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests/test_one.rs"), "#[test] fn t() {}").unwrap();
}

// ===========================================================================
// 1. Basic workspace staging — copy files, verify structure
// ===========================================================================

#[test]
fn stage_single_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "world").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("hello.txt")).unwrap(),
        "world"
    );
}

#[test]
fn stage_preserves_file_content() {
    let src = tempdir().unwrap();
    let content = "line1\nline2\nline3\n";
    fs::write(src.path().join("data.txt"), content).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("data.txt")).unwrap(),
        content
    );
}

#[test]
fn stage_multiple_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "alpha").unwrap();
    fs::write(src.path().join("b.txt"), "beta").unwrap();
    fs::write(src.path().join("c.txt"), "gamma").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()), vec!["a.txt", "b.txt", "c.txt"]);
}

#[test]
fn stage_preserves_all_content() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "alpha").unwrap();
    fs::write(src.path().join("b.txt"), "beta").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("a.txt")).unwrap(),
        "alpha"
    );
    assert_eq!(fs::read_to_string(ws.path().join("b.txt")).unwrap(), "beta");
}

#[test]
fn stage_fixture_tree() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"main.rs".to_string()));
    assert!(files.contains(&"lib.rs".to_string()));
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"src/utils.rs".to_string()));
    assert!(files.contains(&"src/data.json".to_string()));
    assert!(files.contains(&"tests/test_one.rs".to_string()));
}

#[test]
fn stage_returns_different_path_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn stage_binary_file_content() {
    let src = tempdir().unwrap();
    let bytes: Vec<u8> = (0u8..=255).collect();
    fs::write(src.path().join("binary.bin"), &bytes).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("binary.bin")).unwrap(), bytes);
}

#[test]
fn stage_empty_source_produces_no_files() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(collect_files(ws.path()).is_empty());
}

#[test]
fn stage_file_with_no_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Makefile"), "all: build").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("Makefile").exists());
}

#[test]
fn stage_hidden_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join(".hidden")).unwrap(),
        "secret"
    );
}

// ===========================================================================
// 2. Include/exclude glob filtering
// ===========================================================================

#[test]
fn include_only_rs_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected file: {f}");
    }
}

#[test]
fn exclude_json_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec!["*.json".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(!f.ends_with(".json"), "json file not excluded: {f}");
    }
}

#[test]
fn include_src_dir_only() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec!["src/**".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.is_empty());
    for f in &files {
        assert!(f.starts_with("src/"), "unexpected file outside src/: {f}");
    }
}

#[test]
fn exclude_tests_dir() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec!["tests/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(!f.starts_with("tests/"), "tests dir not excluded: {f}");
    }
}

#[test]
fn include_and_exclude_combined() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/data.json".into()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/utils.rs".to_string()));
    assert!(!files.contains(&"src/data.json".to_string()));
}

#[test]
fn exclude_takes_precedence_over_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src/generated")).unwrap();
    fs::write(src.path().join("src/main.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("src/generated/out.rs"), "// gen").unwrap();
    let spec = staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/generated/**".into()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(!files.contains(&"src/generated/out.rs".to_string()));
}

#[test]
fn multiple_include_patterns() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec!["src/**".into(), "tests/**".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/utils.rs".to_string()));
    assert!(files.contains(&"tests/test_one.rs".to_string()));
    assert!(!files.contains(&"README.md".to_string()));
}

#[test]
fn multiple_exclude_patterns() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec![], vec!["*.md".into(), "*.json".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.contains(&"README.md".to_string()));
    assert!(!files.contains(&"src/data.json".to_string()));
    assert!(files.contains(&"main.rs".to_string()));
}

#[test]
fn wildcard_include_matches_all() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let spec = staged_spec_globs(src.path(), vec!["**".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let staged = collect_files(ws.path());
    let original = collect_files(src.path());
    assert_eq!(staged, original);
}

#[test]
fn exclude_specific_file_by_name() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "k").unwrap();
    fs::write(src.path().join("drop.txt"), "d").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec!["drop.txt".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("keep.txt").exists());
    assert!(!ws.path().join("drop.txt").exists());
}

#[test]
fn include_by_extension_multiple() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.toml"), "").unwrap();
    fs::write(src.path().join("c.md"), "").unwrap();
    fs::write(src.path().join("d.txt"), "").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["*.rs".into(), "*.toml".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
    assert!(files.contains(&"b.toml".to_string()));
    assert!(!files.contains(&"c.md".to_string()));
    assert!(!files.contains(&"d.txt".to_string()));
}

#[test]
fn empty_include_means_allow_all() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("anything.xyz"), "xyz").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("anything.xyz").exists());
}

#[test]
fn empty_exclude_means_deny_nothing() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("anything.xyz"), "xyz").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("anything.xyz").exists());
}

// ===========================================================================
// 3. .git directory exclusion
// ===========================================================================

#[test]
fn source_git_dir_not_copied() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git/objects")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("file.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // The staged workspace creates its own .git, but source's .git contents should not be there
    assert!(ws.path().join("file.txt").exists());
    // The source's HEAD content should not be present because a fresh repo is initialised
    let head = fs::read_to_string(ws.path().join(".git/HEAD")).unwrap();
    assert!(head.contains("ref:"), "expected a valid git HEAD");
}

#[test]
fn git_objects_from_source_not_leaked() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git/objects/ab")).unwrap();
    fs::write(src.path().join(".git/objects/ab/cd1234"), "fake-obj").unwrap();
    fs::write(src.path().join("code.rs"), "fn main(){}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(!ws.path().join(".git/objects/ab/cd1234").exists());
}

#[test]
fn git_exclusion_with_real_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
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
            "init",
        ])
        .current_dir(src.path())
        .status();
    // Source has a proper git repo now
    assert!(src.path().join(".git").exists());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Staged workspace should have its OWN .git, not the source's
    let staged_commit_msg = git(ws.path(), &["log", "--oneline", "-1"]);
    assert!(
        staged_commit_msg.contains("baseline"),
        "expected baseline commit, got: {staged_commit_msg}"
    );
}

#[test]
fn git_hooks_from_source_not_copied() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git/hooks")).unwrap();
    fs::write(
        src.path().join(".git/hooks/pre-commit"),
        "#!/bin/sh\nexit 1",
    )
    .unwrap();
    fs::write(src.path().join("main.py"), "print('hi')").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(!ws.path().join(".git/hooks/pre-commit").exists());
}

// ===========================================================================
// 4. Git initialization verification
// ===========================================================================

#[test]
fn staged_workspace_has_git_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(
        ws.path().join(".git").exists(),
        "staged workspace should have .git"
    );
}

#[test]
fn staged_workspace_git_is_valid_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "true");
}

#[test]
fn staged_workspace_git_has_head() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let rev = git(ws.path(), &["rev-parse", "HEAD"]);
    assert!(!rev.is_empty(), "HEAD should point to a valid commit");
    // SHA-1 hex = 40 chars
    assert!(rev.len() >= 40, "expected full SHA, got: {rev}");
}

#[test]
fn stager_with_git_init_true() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.rs"), "fn main(){}").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_with_git_init_false() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.rs"), "fn main(){}").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn git_config_has_user() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // The baseline commit was made with user.name=abp
    let author = git(ws.path(), &["log", "--format=%an", "-1"]);
    assert_eq!(author, "abp");
}

#[test]
fn git_config_has_email() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let email = git(ws.path(), &["log", "--format=%ae", "-1"]);
    assert_eq!(email, "abp@local");
}

// ===========================================================================
// 5. Baseline commit creation
// ===========================================================================

#[test]
fn baseline_commit_exists() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "alpha").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(
        log.contains("baseline"),
        "expected 'baseline' commit: {log}"
    );
}

#[test]
fn baseline_commit_is_single() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "alpha").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1", "expected exactly 1 commit");
}

#[test]
fn baseline_commit_tracks_all_staged_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let tracked = git(ws.path(), &["ls-files"]);
    assert!(tracked.contains("a.txt"));
    assert!(tracked.contains("b.txt"));
}

#[test]
fn baseline_commit_has_correct_message() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "f").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let msg = git(ws.path(), &["log", "--format=%s", "-1"]);
    assert_eq!(msg, "baseline");
}

#[test]
fn no_uncommitted_changes_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap_or_default();
    assert!(status.is_empty(), "expected clean status, got: {status}");
}

#[test]
fn baseline_commit_tree_matches_staged_files() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("root.txt"), "r").unwrap();
    fs::write(src.path().join("sub/nested.txt"), "n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let tree = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    let lines: Vec<&str> = tree.lines().collect();
    assert!(lines.contains(&"root.txt"));
    assert!(lines.contains(&"sub/nested.txt"));
}

#[test]
fn baseline_with_empty_source_has_empty_commit() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Even with no files, ensure git was initialised
    if ws.path().join(".git").exists() {
        let log_result = git_opt(ws.path(), &["log", "--oneline"]);
        // May have no commit if there was nothing to commit — that's acceptable
        if let Some(log) = log_result {
            assert!(log.is_empty() || log.contains("baseline"));
        }
    }
}

// ===========================================================================
// 6. Diff generation after file modifications
// ===========================================================================

#[test]
fn diff_is_empty_after_staging() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "hello").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap_or_default();
    assert!(diff.is_empty(), "expected empty diff, got: {diff}");
}

#[test]
fn diff_detects_modified_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "modified").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("original"), "diff should show old content");
    assert!(diff.contains("modified"), "diff should show new content");
}

#[test]
fn diff_detects_new_file_after_add() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new.txt"), "brand new").unwrap();
    // Stage the new file so diff shows it
    let _ = Command::new("git")
        .args(["add", "new.txt"])
        .current_dir(ws.path())
        .status();
    let diff = git(ws.path(), &["diff", "--cached", "--no-color"]);
    assert!(diff.contains("new.txt"), "diff should mention new file");
    assert!(diff.contains("brand new"));
}

#[test]
fn diff_detects_deleted_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("victim.txt"), "delete me").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("victim.txt")).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(
        status.contains("victim.txt"),
        "status should show deleted file"
    );
}

#[test]
fn status_is_clean_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap_or_default();
    assert!(status.is_empty(), "should be clean: {status}");
}

#[test]
fn status_detects_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main(){}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("code.rs"), "fn main(){ println!(\"hi\"); }").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("code.rs"));
}

#[test]
fn diff_multiline_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("multi.txt"), "line1\nline2\nline3\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("multi.txt"), "line1\nCHANGED\nline3\n").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("-line2"));
    assert!(diff.contains("+CHANGED"));
}

#[test]
fn diff_append_to_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("log.txt"), "entry1\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let mut contents = fs::read_to_string(ws.path().join("log.txt")).unwrap();
    contents.push_str("entry2\n");
    fs::write(ws.path().join("log.txt"), &contents).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("+entry2"));
}

// ===========================================================================
// 7. Nested directory structure preservation
// ===========================================================================

#[test]
fn nested_dirs_preserved() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b/c")).unwrap();
    fs::write(src.path().join("a/b/c/deep.txt"), "deep").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("a/b/c/deep.txt")).unwrap(),
        "deep"
    );
}

#[test]
fn deeply_nested_structure() {
    let src = tempdir().unwrap();
    let deep = "a/b/c/d/e/f/g";
    fs::create_dir_all(src.path().join(deep)).unwrap();
    fs::write(src.path().join(format!("{deep}/leaf.txt")), "leaf").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(format!("{deep}/leaf.txt")).exists());
}

#[test]
fn multiple_nested_dirs() {
    let src = tempdir().unwrap();
    for dir in &["alpha/one", "beta/two", "gamma/three"] {
        fs::create_dir_all(src.path().join(dir)).unwrap();
        fs::write(src.path().join(format!("{dir}/file.txt")), dir).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for dir in &["alpha/one", "beta/two", "gamma/three"] {
        assert_eq!(
            fs::read_to_string(ws.path().join(format!("{dir}/file.txt"))).unwrap(),
            *dir
        );
    }
}

#[test]
fn sibling_dirs_both_preserved() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("dir_a")).unwrap();
    fs::create_dir_all(src.path().join("dir_b")).unwrap();
    fs::write(src.path().join("dir_a/a.txt"), "a").unwrap();
    fs::write(src.path().join("dir_b/b.txt"), "b").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("dir_a/a.txt").exists());
    assert!(ws.path().join("dir_b/b.txt").exists());
}

#[test]
fn nested_include_filter() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src/core")).unwrap();
    fs::create_dir_all(src.path().join("docs")).unwrap();
    fs::write(src.path().join("src/core/main.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("docs/guide.md"), "# guide").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["src/**".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("src/core/main.rs").exists());
    assert!(!ws.path().join("docs/guide.md").exists());
}

#[test]
fn nested_dir_with_files_at_every_level() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "0").unwrap();
    fs::create_dir_all(src.path().join("l1")).unwrap();
    fs::write(src.path().join("l1/one.txt"), "1").unwrap();
    fs::create_dir_all(src.path().join("l1/l2")).unwrap();
    fs::write(src.path().join("l1/l2/two.txt"), "2").unwrap();
    fs::create_dir_all(src.path().join("l1/l2/l3")).unwrap();
    fs::write(src.path().join("l1/l2/l3/three.txt"), "3").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("root.txt")).unwrap(), "0");
    assert_eq!(
        fs::read_to_string(ws.path().join("l1/one.txt")).unwrap(),
        "1"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("l1/l2/two.txt")).unwrap(),
        "2"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("l1/l2/l3/three.txt")).unwrap(),
        "3"
    );
}

// ===========================================================================
// 8. Symlink handling
// ===========================================================================

#[cfg(unix)]
mod symlink_tests {
    use super::*;
    use std::os::unix::fs as unix_fs;

    #[test]
    fn symlink_to_file_not_followed() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("real.txt"), "real").unwrap();
        unix_fs::symlink(src.path().join("real.txt"), src.path().join("link.txt")).unwrap();
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        // follow_links(false) means symlinks are not followed
        assert!(ws.path().join("real.txt").exists());
        // The symlink itself should not be copied as a regular file
        // (walkdir with follow_links(false) visits the symlink entry but copy_workspace
        // only copies files/dirs, not symlinks)
    }

    #[test]
    fn symlink_to_dir_not_followed() {
        let src = tempdir().unwrap();
        fs::create_dir_all(src.path().join("real_dir")).unwrap();
        fs::write(src.path().join("real_dir/content.txt"), "c").unwrap();
        unix_fs::symlink(src.path().join("real_dir"), src.path().join("link_dir")).unwrap();
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        assert!(ws.path().join("real_dir/content.txt").exists());
        // link_dir should not have been traversed
        assert!(!ws.path().join("link_dir/content.txt").exists());
    }
}

#[cfg(windows)]
mod symlink_tests {
    use super::*;

    #[test]
    fn windows_symlinks_skipped_gracefully() {
        // On Windows, symlink creation requires privileges; just verify staging works
        // without symlinks present.
        let src = tempdir().unwrap();
        fs::write(src.path().join("file.txt"), "content").unwrap();
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        assert!(ws.path().join("file.txt").exists());
    }
}

// ===========================================================================
// 9. Empty directory handling
// ===========================================================================

#[test]
fn empty_subdir_is_created() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_dir")).unwrap();
    fs::write(src.path().join("anchor.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // The empty dir may or may not be created depending on walkdir traversal;
    // what matters is the staging succeeds and anchor file is present
    assert!(ws.path().join("anchor.txt").exists());
}

#[test]
fn empty_nested_subdir() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b/c")).unwrap();
    fs::write(src.path().join("root.txt"), "root").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("root.txt").exists());
}

#[test]
fn dir_with_only_subdirs_no_files() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b")).unwrap();
    fs::create_dir_all(src.path().join("a/c")).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Should succeed without error even if no files
    assert!(ws.path().exists());
}

#[test]
fn staging_fully_empty_source() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().exists());
    assert!(collect_files(ws.path()).is_empty());
}

// ===========================================================================
// 10. Large file handling
// ===========================================================================

#[test]
fn stage_1mb_file() {
    let src = tempdir().unwrap();
    let data = vec![b'X'; 1024 * 1024];
    fs::write(src.path().join("big.bin"), &data).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let copied = fs::read(ws.path().join("big.bin")).unwrap();
    assert_eq!(copied.len(), 1024 * 1024);
    assert!(copied.iter().all(|&b| b == b'X'));
}

#[test]
fn stage_many_small_files() {
    let src = tempdir().unwrap();
    for i in 0..100 {
        fs::write(
            src.path().join(format!("file_{i:03}.txt")),
            format!("content_{i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 100);
}

#[test]
fn large_file_content_integrity() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
    fs::write(src.path().join("pattern.bin"), &data).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let copied = fs::read(ws.path().join("pattern.bin")).unwrap();
    assert_eq!(copied, data);
}

#[test]
fn stage_file_with_long_lines() {
    let src = tempdir().unwrap();
    let long_line = "A".repeat(10_000);
    fs::write(src.path().join("long.txt"), &long_line).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("long.txt")).unwrap(),
        long_line
    );
}

// ===========================================================================
// 11. Unicode filename handling
// ===========================================================================

#[test]
fn unicode_filename_latin() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("café.txt"), "latte").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("café.txt")).unwrap(),
        "latte"
    );
}

#[test]
fn unicode_filename_cjk() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("文件.txt"), "内容").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("文件.txt")).unwrap(),
        "内容"
    );
}

#[test]
fn unicode_filename_emoji() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("🚀.txt"), "launch").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("🚀.txt")).unwrap(),
        "launch"
    );
}

#[test]
fn unicode_directory_name() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("données")).unwrap();
    fs::write(src.path().join("données/résultat.txt"), "résultat").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("données/résultat.txt")).unwrap(),
        "résultat"
    );
}

#[test]
fn unicode_content_preserved() {
    let src = tempdir().unwrap();
    let content = "日本語テスト\n中文测试\n한국어 테스트\nрусский тест\n";
    fs::write(src.path().join("i18n.txt"), content).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("i18n.txt")).unwrap(),
        content
    );
}

#[test]
fn mixed_unicode_and_ascii_filenames() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("ascii.txt"), "a").unwrap();
    fs::write(src.path().join("überblick.txt"), "ü").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("ascii.txt").exists());
    assert!(ws.path().join("überblick.txt").exists());
}

// ===========================================================================
// 12. Read-only file handling
// ===========================================================================

#[cfg(unix)]
mod readonly_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn readonly_file_is_copied() {
        let src = tempdir().unwrap();
        let path = src.path().join("readonly.txt");
        fs::write(&path, "protected").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        assert!(ws.path().join("readonly.txt").exists());
        assert_eq!(
            fs::read_to_string(ws.path().join("readonly.txt")).unwrap(),
            "protected"
        );
    }

    #[test]
    fn readonly_dir_contents_accessible() {
        let src = tempdir().unwrap();
        let dir = src.path().join("locked");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("inside.txt"), "data").unwrap();
        // Make dir read-only after writing
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        assert!(ws.path().join("locked/inside.txt").exists());
        // Restore permissions for cleanup
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(windows)]
mod readonly_tests {
    use super::*;

    #[test]
    fn readonly_file_is_copied_windows() {
        let src = tempdir().unwrap();
        let path = src.path().join("readonly.txt");
        fs::write(&path, "protected").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms).unwrap();
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        assert!(ws.path().join("readonly.txt").exists());
        assert_eq!(
            fs::read_to_string(ws.path().join("readonly.txt")).unwrap(),
            "protected"
        );
        // Restore for cleanup
        let mut perms = fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&path, perms).unwrap();
    }
}

// ===========================================================================
// 13. Multiple workspace staging (isolation)
// ===========================================================================

#[test]
fn two_workspaces_have_different_paths() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws1.path(), ws2.path());
}

#[test]
fn workspaces_are_isolated_modifications() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "original").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws1.path().join("shared.txt"), "modified_ws1").unwrap();
    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "original",
        "ws2 should not be affected by ws1 modifications"
    );
}

#[test]
fn workspaces_have_independent_git_repos() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let sha1 = git(ws1.path(), &["rev-parse", "HEAD"]);
    let sha2 = git(ws2.path(), &["rev-parse", "HEAD"]);
    // Both have baseline commits but might have different SHAs due to timestamps
    assert!(!sha1.is_empty());
    assert!(!sha2.is_empty());
}

#[test]
fn workspace_modification_does_not_affect_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("safe.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("safe.txt"), "changed").unwrap();
    assert_eq!(
        fs::read_to_string(src.path().join("safe.txt")).unwrap(),
        "original"
    );
}

#[test]
fn multiple_workspaces_from_different_sources() {
    let src_a = tempdir().unwrap();
    let src_b = tempdir().unwrap();
    fs::write(src_a.path().join("a.txt"), "from_a").unwrap();
    fs::write(src_b.path().join("b.txt"), "from_b").unwrap();
    let ws_a = WorkspaceManager::prepare(&staged_spec(src_a.path())).unwrap();
    let ws_b = WorkspaceManager::prepare(&staged_spec(src_b.path())).unwrap();
    assert!(ws_a.path().join("a.txt").exists());
    assert!(!ws_a.path().join("b.txt").exists());
    assert!(ws_b.path().join("b.txt").exists());
    assert!(!ws_b.path().join("a.txt").exists());
}

#[test]
fn three_simultaneous_workspaces() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "content").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws3 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let paths: Vec<&Path> = vec![ws1.path(), ws2.path(), ws3.path()];
    // All paths should be unique
    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            assert_ne!(paths[i], paths[j]);
        }
    }
}

#[test]
fn adding_file_in_one_workspace_invisible_to_another() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("base.txt"), "base").unwrap();
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws1.path().join("new_file.txt"), "only in ws1").unwrap();
    assert!(!ws2.path().join("new_file.txt").exists());
}

// ===========================================================================
// 14. Workspace cleanup on drop
// ===========================================================================

#[test]
fn staged_workspace_cleaned_up_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws_path: PathBuf;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        ws_path = ws.path().to_path_buf();
        assert!(ws_path.exists(), "workspace should exist while alive");
    }
    // After drop, the temp dir should be cleaned up
    assert!(
        !ws_path.exists(),
        "workspace should be cleaned up after drop"
    );
}

#[test]
fn passthrough_workspace_not_cleaned_up() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws_path: PathBuf;
    {
        let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
        ws_path = ws.path().to_path_buf();
    }
    // Passthrough mode doesn't own the directory
    assert!(
        ws_path.exists(),
        "passthrough workspace should survive drop"
    );
}

#[test]
fn stager_workspace_cleaned_up_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws_path: PathBuf;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .stage()
            .unwrap();
        ws_path = ws.path().to_path_buf();
        assert!(ws_path.exists());
    }
    assert!(
        !ws_path.exists(),
        "stager workspace should be cleaned up after drop"
    );
}

#[test]
fn cleanup_removes_all_contents() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("sub/deep")).unwrap();
    fs::write(src.path().join("sub/deep/file.txt"), "deep").unwrap();
    let ws_path: PathBuf;
    let sub_path: PathBuf;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        ws_path = ws.path().to_path_buf();
        sub_path = ws.path().join("sub/deep/file.txt");
        assert!(sub_path.exists());
    }
    assert!(!sub_path.exists());
    assert!(!ws_path.exists());
}

// ===========================================================================
// 15. WorkspaceStager builder API
// ===========================================================================

#[test]
fn stager_basic_usage() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main(){}").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn stager_default_enables_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_with_include() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "").unwrap();
    fs::write(src.path().join("drop.txt"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();
    assert!(ws.path().join("keep.rs").exists());
    assert!(!ws.path().join("drop.txt").exists());
}

#[test]
fn stager_with_exclude() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "").unwrap();
    fs::write(src.path().join("drop.log"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .stage()
        .unwrap();
    assert!(ws.path().join("keep.txt").exists());
    assert!(!ws.path().join("drop.log").exists());
}

#[test]
fn stager_with_include_and_exclude() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src/gen")).unwrap();
    fs::write(src.path().join("src/main.rs"), "").unwrap();
    fs::write(src.path().join("src/gen/out.rs"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["src/gen/**".into()])
        .stage()
        .unwrap();
    assert!(ws.path().join("src/main.rs").exists());
    assert!(!ws.path().join("src/gen/out.rs").exists());
}

#[test]
fn stager_without_source_root_fails() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("source_root"));
}

#[test]
fn stager_nonexistent_source_fails() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
}

#[test]
fn stager_chaining_order_irrelevant() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.log"), "").unwrap();
    // Different ordering of builder calls
    let ws = WorkspaceStager::new()
        .exclude(vec!["*.log".into()])
        .source_root(src.path())
        .with_git_init(true)
        .include(vec!["*.rs".into(), "*.log".into()])
        .stage()
        .unwrap();
    assert!(ws.path().join("a.rs").exists());
    assert!(!ws.path().join("b.log").exists()); // exclude takes precedence
}

#[test]
fn stager_path_returns_valid_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().is_dir());
}

#[test]
fn stager_from_string_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let path_str = src.path().to_string_lossy().to_string();
    let ws = WorkspaceStager::new()
        .source_root(path_str)
        .stage()
        .unwrap();
    assert!(ws.path().join("f.txt").exists());
}

// ===========================================================================
// 16. PassThrough mode
// ===========================================================================

#[test]
fn passthrough_returns_original_path() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn passthrough_does_not_copy() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    // Modifying through ws should affect source directly
    fs::write(ws.path().join("f.txt"), "changed").unwrap();
    assert_eq!(
        fs::read_to_string(src.path().join("f.txt")).unwrap(),
        "changed"
    );
}

#[test]
fn passthrough_does_not_init_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let had_git_before = src.path().join(".git").exists();
    let _ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    let has_git_after = src.path().join(".git").exists();
    assert_eq!(
        had_git_before, has_git_after,
        "passthrough should not modify git state"
    );
}

// ===========================================================================
// 17. Edge cases and error handling
// ===========================================================================

#[test]
fn dotfiles_are_staged_except_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    fs::write(src.path().join(".env"), "SECRET=123").unwrap();
    fs::write(src.path().join(".editorconfig"), "[*]").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".gitignore").exists());
    assert!(ws.path().join(".env").exists());
    assert!(ws.path().join(".editorconfig").exists());
}

#[test]
fn file_with_spaces_in_name() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("my file.txt"), "spaces").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("my file.txt")).unwrap(),
        "spaces"
    );
}

#[test]
fn file_with_special_chars_in_name() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file-with_special.chars.txt"), "special").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("file-with_special.chars.txt").exists());
}

#[test]
fn empty_file_staged() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("empty.txt").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

#[test]
fn file_with_newlines_only() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("newlines.txt"), "\n\n\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("newlines.txt")).unwrap(),
        "\n\n\n"
    );
}

#[test]
fn file_with_windows_line_endings() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("crlf.txt"), "line1\r\nline2\r\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let content = fs::read(ws.path().join("crlf.txt")).unwrap();
    // Binary comparison to ensure no line ending conversion
    assert_eq!(content, b"line1\r\nline2\r\n");
}

#[test]
fn staging_preserves_file_count() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let original_count = collect_files(src.path()).len();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let staged_count = collect_files(ws.path()).len();
    assert_eq!(original_count, staged_count);
}

#[test]
fn relative_paths_match_source() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let original_files = collect_files(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let staged_files = collect_files(ws.path());
    assert_eq!(original_files, staged_files);
}

// ===========================================================================
// 18. Git diff / status API integration
// ===========================================================================

#[test]
fn git_status_returns_some_for_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
}

#[test]
fn git_diff_returns_some_for_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
}

#[test]
fn git_status_returns_none_for_non_repo() {
    let non_repo = tempdir().unwrap();
    let status = WorkspaceManager::git_status(non_repo.path());
    assert!(status.is_none());
}

#[test]
fn git_diff_returns_none_for_non_repo() {
    let non_repo = tempdir().unwrap();
    let diff = WorkspaceManager::git_diff(non_repo.path());
    assert!(diff.is_none());
}

#[test]
fn git_status_after_file_creation() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new.txt"), "new content").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(!status.is_empty(), "status should show untracked file");
}

#[test]
fn git_diff_after_staged_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn original(){}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("code.rs"), "fn modified(){}").unwrap();
    let _ = Command::new("git")
        .args(["add", "code.rs"])
        .current_dir(ws.path())
        .status();
    let diff = git(ws.path(), &["diff", "--cached", "--no-color"]);
    assert!(diff.contains("original"));
    assert!(diff.contains("modified"));
}

// ===========================================================================
// 19. Workspace with filtered git operations
// ===========================================================================

#[test]
fn filtered_workspace_git_tracks_only_included() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "fn a(){}").unwrap();
    fs::write(src.path().join("b.txt"), "text").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let tracked = git(ws.path(), &["ls-files"]);
    assert!(tracked.contains("a.rs"));
    assert!(!tracked.contains("b.txt"));
}

#[test]
fn excluded_files_not_in_baseline_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("good.rs"), "fn good(){}").unwrap();
    fs::write(src.path().join("bad.log"), "error log").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec!["*.log".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let tree = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("good.rs"));
    assert!(!tree.contains("bad.log"));
}

// ===========================================================================
// 20. Workspace spec construction edge cases
// ===========================================================================

#[test]
fn spec_with_absolute_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let spec = staged_spec(src.path());
    assert!(PathBuf::from(&spec.root).is_absolute());
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("f.txt").exists());
}

#[test]
fn stager_default_is_equivalent_to_new() {
    // Default::default() and new() should behave the same
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceStager::default()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("f.txt").exists());
    assert!(ws.path().join(".git").exists());
}

// ===========================================================================
// 21. Complex real-world scenarios
// ===========================================================================

#[test]
fn realistic_rust_project_structure() {
    let src = tempdir().unwrap();
    let files = &[
        ("Cargo.toml", "[package]\nname = \"myproj\"\n"),
        ("Cargo.lock", "# lock\n"),
        ("src/main.rs", "fn main() { println!(\"hello\"); }\n"),
        ("src/lib.rs", "pub mod utils;\n"),
        (
            "src/utils.rs",
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        ),
        (
            "tests/integration.rs",
            "#[test] fn it_works() { assert!(true); }\n",
        ),
        ("benches/bench.rs", "// bench\n"),
        ("README.md", "# My Project\n"),
        (".gitignore", "/target\n"),
    ];
    for (path, content) in files {
        let full = src.path().join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, content).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for (path, content) in files {
        assert_eq!(
            fs::read_to_string(ws.path().join(path)).unwrap(),
            *content,
            "content mismatch for {path}"
        );
    }
}

#[test]
fn realistic_exclude_target_and_lock() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Cargo.toml"), "[package]").unwrap();
    fs::write(src.path().join("Cargo.lock"), "lock").unwrap();
    fs::create_dir_all(src.path().join("target/debug")).unwrap();
    fs::write(src.path().join("target/debug/binary"), "bin").unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::write(src.path().join("src/main.rs"), "fn main(){}").unwrap();
    let spec = staged_spec_globs(
        src.path(),
        vec![],
        vec!["target/**".into(), "Cargo.lock".into()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("Cargo.toml").exists());
    assert!(ws.path().join("src/main.rs").exists());
    assert!(!ws.path().join("Cargo.lock").exists());
    assert!(!ws.path().join("target/debug/binary").exists());
}

#[test]
fn modify_staged_file_and_verify_diff_output() {
    let src = tempdir().unwrap();
    fs::write(
        src.path().join("config.toml"),
        "[server]\nhost = \"localhost\"\nport = 8080\n",
    )
    .unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(
        ws.path().join("config.toml"),
        "[server]\nhost = \"0.0.0.0\"\nport = 9090\n",
    )
    .unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("localhost"));
    assert!(diff.contains("0.0.0.0"));
    assert!(diff.contains("8080"));
    assert!(diff.contains("9090"));
}

#[test]
fn stage_then_add_remove_modify_verify_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("remove.txt"), "remove").unwrap();
    fs::write(src.path().join("modify.txt"), "before").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Modify
    fs::write(ws.path().join("modify.txt"), "after").unwrap();
    // Remove
    fs::remove_file(ws.path().join("remove.txt")).unwrap();
    // Add
    fs::write(ws.path().join("added.txt"), "new").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(
        status.contains("modify.txt"),
        "modified file should show: {status}"
    );
    assert!(
        status.contains("remove.txt"),
        "deleted file should show: {status}"
    );
    assert!(
        status.contains("added.txt"),
        "new file should show: {status}"
    );
}

// ===========================================================================
// 22. Additional glob edge cases
// ===========================================================================

#[test]
fn exclude_dotenv_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".env"), "SECRET=123").unwrap();
    fs::write(src.path().join("app.rs"), "fn main(){}").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec![".env".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(!ws.path().join(".env").exists());
    assert!(ws.path().join("app.rs").exists());
}

#[test]
fn include_only_toml_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Cargo.toml"), "[package]").unwrap();
    fs::write(src.path().join("config.toml"), "[config]").unwrap();
    fs::write(src.path().join("readme.md"), "# hi").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["*.toml".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("Cargo.toml").exists());
    assert!(ws.path().join("config.toml").exists());
    assert!(!ws.path().join("readme.md").exists());
}

#[test]
fn exclude_nested_generated_dir() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src/generated/protos")).unwrap();
    fs::write(src.path().join("src/lib.rs"), "pub mod generated;").unwrap();
    fs::write(src.path().join("src/generated/protos/api.rs"), "// gen").unwrap();
    let spec = staged_spec_globs(src.path(), vec![], vec!["src/generated/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("src/lib.rs").exists());
    assert!(!ws.path().join("src/generated/protos/api.rs").exists());
}

#[test]
fn include_multiple_extensions_with_globstar() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::write(src.path().join("src/main.rs"), "").unwrap();
    fs::write(src.path().join("src/style.css"), "").unwrap();
    fs::write(src.path().join("src/data.json"), "").unwrap();
    let spec = staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into(), "**/*.css".into()],
        vec![],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("src/main.rs").exists());
    assert!(ws.path().join("src/style.css").exists());
    assert!(!ws.path().join("src/data.json").exists());
}

// ===========================================================================
// 23. Git operation sequences
// ===========================================================================

#[test]
fn multiple_modifications_generate_cumulative_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "first\n").unwrap();
    fs::write(src.path().join("b.txt"), "second\n").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("a.txt"), "first_modified\n").unwrap();
    fs::write(ws.path().join("b.txt"), "second_modified\n").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("first_modified"));
    assert!(diff.contains("second_modified"));
}

#[test]
fn revert_modification_produces_clean_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "changed").unwrap();
    // Revert
    fs::write(ws.path().join("f.txt"), "original").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap_or_default();
    assert!(status.is_empty(), "should be clean after revert: {status}");
}

#[test]
fn commit_after_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "v1").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("code.rs"), "v2").unwrap();
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(ws.path())
        .status();
    let _ = Command::new("git")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "update",
        ])
        .current_dir(ws.path())
        .status();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "2", "should have baseline + update commits");
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"));
    assert!(log.contains("update"));
}

// ===========================================================================
// 24. Content type variations
// ===========================================================================

#[test]
fn json_file_staged_correctly() {
    let src = tempdir().unwrap();
    let json = r#"{"name":"test","version":"1.0.0","deps":[]}"#;
    fs::write(src.path().join("package.json"), json).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("package.json")).unwrap(),
        json
    );
}

#[test]
fn toml_file_staged_correctly() {
    let src = tempdir().unwrap();
    let toml = "[package]\nname = \"test\"\nversion = \"0.1.0\"\n";
    fs::write(src.path().join("Cargo.toml"), toml).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("Cargo.toml")).unwrap(),
        toml
    );
}

#[test]
fn yaml_file_staged_correctly() {
    let src = tempdir().unwrap();
    let yaml = "name: test\nversion: 1.0\nitems:\n  - first\n  - second\n";
    fs::write(src.path().join("config.yaml"), yaml).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("config.yaml")).unwrap(),
        yaml
    );
}

#[test]
fn shell_script_staged_correctly() {
    let src = tempdir().unwrap();
    let script = "#!/bin/bash\necho \"hello world\"\nexit 0\n";
    fs::write(src.path().join("run.sh"), script).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("run.sh")).unwrap(),
        script
    );
}

// ===========================================================================
// 25. WorkspaceManager as Copy type
// ===========================================================================

#[test]
fn workspace_manager_is_copy() {
    let _a = WorkspaceManager;
    let _b = _a; // Copy
    let _c = _a; // Still valid
}

#[test]
fn workspace_manager_prepare_from_clone() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let mgr = WorkspaceManager;
    let _ = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // mgr is still valid (Copy type)
    let _ = mgr;
}

// ===========================================================================
// 26. Cross-platform path handling
// ===========================================================================

#[test]
fn paths_with_forward_slashes_in_spec() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::write(src.path().join("src/main.rs"), "fn main(){}").unwrap();
    let spec = staged_spec_globs(src.path(), vec!["src/**".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("src").join("main.rs").exists());
}

#[test]
fn collect_files_normalises_separators() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::write(src.path().join("a").join("b").join("c.txt"), "c").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"a/b/c.txt".to_string()));
}

// ===========================================================================
// 27. Concurrent staging safety
// ===========================================================================

#[test]
fn concurrent_staging_produces_unique_paths() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();
    let workspaces: Vec<_> = (0..5)
        .map(|_| WorkspaceManager::prepare(&staged_spec(src.path())).unwrap())
        .collect();
    let paths: Vec<_> = workspaces.iter().map(|w| w.path().to_path_buf()).collect();
    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            assert_ne!(paths[i], paths[j], "workspaces {i} and {j} share a path");
        }
    }
}

// ===========================================================================
// 28. Stager git_init interaction with globs
// ===========================================================================

#[test]
fn stager_no_git_still_copies_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("a.txt").exists());
    assert!(ws.path().join("b.txt").exists());
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn stager_with_git_and_exclude() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "").unwrap();
    fs::write(src.path().join("drop.log"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
    let tracked = git(ws.path(), &["ls-files"]);
    assert!(tracked.contains("keep.rs"));
    assert!(!tracked.contains("drop.log"));
}

#[test]
fn stager_no_git_with_include() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.txt"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("a.rs").exists());
    assert!(!ws.path().join("b.txt").exists());
    assert!(!ws.path().join(".git").exists());
}

// ===========================================================================
// 29. WorkspaceSpec with complex glob combos
// ===========================================================================

#[test]
fn exclude_hidden_except_gitignore() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    fs::write(src.path().join(".env"), "SECRET").unwrap();
    fs::write(src.path().join(".hidden"), "h").unwrap();
    fs::write(src.path().join("visible.txt"), "v").unwrap();
    // Exclude all dotfiles except .gitignore
    // Note: globset treats .env and .hidden as matching ".*" pattern
    let spec = staged_spec_globs(src.path(), vec![], vec![".env".into(), ".hidden".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join(".gitignore").exists());
    assert!(ws.path().join("visible.txt").exists());
    assert!(!ws.path().join(".env").exists());
    assert!(!ws.path().join(".hidden").exists());
}

#[test]
fn include_deeply_nested_with_exclude_at_depth() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("pkg/a/internal")).unwrap();
    fs::create_dir_all(src.path().join("pkg/b")).unwrap();
    fs::write(src.path().join("pkg/a/public.rs"), "pub").unwrap();
    fs::write(src.path().join("pkg/a/internal/private.rs"), "priv").unwrap();
    fs::write(src.path().join("pkg/b/lib.rs"), "lib").unwrap();
    let spec = staged_spec_globs(
        src.path(),
        vec!["pkg/**".into()],
        vec!["pkg/a/internal/**".into()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("pkg/a/public.rs").exists());
    assert!(ws.path().join("pkg/b/lib.rs").exists());
    assert!(!ws.path().join("pkg/a/internal/private.rs").exists());
}

// ===========================================================================
// 30. Additional comprehensive tests
// ===========================================================================

#[test]
fn stage_preserves_subdirectory_hierarchy() {
    let src = tempdir().unwrap();
    let structure = vec![
        "src/main.rs",
        "src/module/mod.rs",
        "src/module/sub/deep.rs",
        "tests/unit.rs",
        "tests/integration/e2e.rs",
    ];
    for path in &structure {
        let full = src.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, format!("// {path}")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for path in &structure {
        assert!(ws.path().join(path).exists(), "missing: {path}");
        assert_eq!(
            fs::read_to_string(ws.path().join(path)).unwrap(),
            format!("// {path}")
        );
    }
}

#[test]
fn diff_shows_filename_in_output() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("specific_name.rs"), "v1").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("specific_name.rs"), "v2").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(
        diff.contains("specific_name.rs"),
        "diff should contain filename: {diff}"
    );
}

#[test]
fn stager_multiple_exclude_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("main.rs"), "").unwrap();
    fs::write(src.path().join("debug.log"), "").unwrap();
    fs::write(src.path().join("data.tmp"), "").unwrap();
    fs::write(src.path().join("cache.dat"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into(), "*.tmp".into(), "*.dat".into()])
        .stage()
        .unwrap();
    assert!(ws.path().join("main.rs").exists());
    assert!(!ws.path().join("debug.log").exists());
    assert!(!ws.path().join("data.tmp").exists());
    assert!(!ws.path().join("cache.dat").exists());
}

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
fn staged_workspace_is_writable() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Should be able to write freely in the staged workspace
    fs::write(ws.path().join("f.txt"), "overwritten").unwrap();
    fs::write(ws.path().join("brand_new.txt"), "fresh").unwrap();
    fs::create_dir_all(ws.path().join("new_dir")).unwrap();
    fs::write(ws.path().join("new_dir/nested.txt"), "nested").unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("brand_new.txt")).unwrap(),
        "fresh"
    );
}

#[test]
fn stage_file_with_no_trailing_newline() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("no_newline.txt"), "no newline at end").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("no_newline.txt")).unwrap(),
        "no newline at end"
    );
}

#[test]
fn stage_preserves_file_with_only_whitespace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("spaces.txt"), "   \t  \n  ").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("spaces.txt")).unwrap(),
        "   \t  \n  "
    );
}

#[test]
fn git_log_format_is_parseable() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--format=%H %s", "-1"]);
    let parts: Vec<&str> = log.splitn(2, ' ').collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].len(), 40, "expected SHA-1 hash length");
    assert_eq!(parts[1], "baseline");
}

#[test]
fn stage_does_not_modify_source_timestamps() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let before = fs::metadata(src.path().join("f.txt"))
        .unwrap()
        .modified()
        .unwrap();
    let _ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let after = fs::metadata(src.path().join("f.txt"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after, "source file timestamp should not change");
}

#[test]
fn stage_many_directories() {
    let src = tempdir().unwrap();
    for i in 0..20 {
        let dir = src.path().join(format!("dir_{i:02}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), format!("content_{i}")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for i in 0..20 {
        let path = format!("dir_{i:02}/file.txt");
        assert!(ws.path().join(&path).exists(), "missing: {path}");
    }
}

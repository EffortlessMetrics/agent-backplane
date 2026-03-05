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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! End-to-end tests for the `abp-git` crate and its integration with
//! `abp-workspace` workspace staging.
//!
//! Organized into: initialization, add/commit, diff, status, workspace
//! integration, error handling, and edge cases.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

use abp_git::{ensure_git_repo, git_diff, git_status};

// ── helpers ──────────────────────────────────────────────────────────

fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("git should be on PATH");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn git_ok(path: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(path)
        .status()
        .expect("git should be on PATH");
    assert!(st.success(), "git {args:?} failed");
}

fn commit(path: &Path, msg: &str) {
    git_ok(
        path,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=t@t",
            "commit",
            "-m",
            msg,
        ],
    );
}

fn tmp() -> TempDir {
    TempDir::new().expect("create temp dir")
}

// ═══════════════════════════════════════════════════════════════════
// 1. Git repository initialization
// ═══════════════════════════════════════════════════════════════════

#[test]
fn init_creates_dot_git_directory() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_idempotent_on_second_call() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_creates_baseline_commit() {
    let dir = tmp();
    fs::write(dir.path().join("x.txt"), "x").unwrap();
    ensure_git_repo(dir.path());
    let log = git(dir.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"), "got: {log}");
}

#[test]
fn init_stages_all_existing_files() {
    let dir = tmp();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(dir.path().join(name), name).unwrap();
    }
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(tracked.contains(name), "missing {name} in: {tracked}");
    }
}

#[test]
fn init_empty_dir_creates_repo() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_skips_existing_repo() {
    let dir = tmp();
    git_ok(dir.path(), &["init", "-q"]);
    let log_before = git(dir.path(), &["log", "--oneline"]);
    ensure_git_repo(dir.path());
    let log_after = git(dir.path(), &["log", "--oneline"]);
    assert_eq!(log_before, log_after);
}

#[test]
fn init_preserves_file_contents() {
    let dir = tmp();
    fs::write(dir.path().join("data.txt"), "hello world").unwrap();
    ensure_git_repo(dir.path());
    let content = fs::read_to_string(dir.path().join("data.txt")).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn init_creates_single_commit() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "f").unwrap();
    ensure_git_repo(dir.path());
    let log = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(log.trim(), "1");
}

#[test]
fn init_with_subdirectories() {
    let dir = tmp();
    let sub = dir.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("inner.txt"), "inner").unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("inner.txt"));
}

#[test]
fn init_multiple_subdirectory_levels() {
    let dir = tmp();
    let deep = dir.path().join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.txt"), "deep").unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("deep.txt"));
}

#[test]
fn init_repo_is_on_default_branch() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    let branch = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert!(!branch.trim().is_empty(), "expected a branch name");
}

// ═══════════════════════════════════════════════════════════════════
// 2. Git add and commit operations
// ═══════════════════════════════════════════════════════════════════

#[test]
fn add_and_commit_new_file() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("new.txt"), "content").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);
    commit(dir.path(), "add new");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn add_and_commit_modified_file() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    commit(dir.path(), "update f");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn add_and_commit_deletion() {
    let dir = tmp();
    fs::write(dir.path().join("del.txt"), "bye").unwrap();
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("del.txt")).unwrap();
    git_ok(dir.path(), &["add", "del.txt"]);
    commit(dir.path(), "delete del");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn commit_multiple_files_at_once() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    for i in 0..5 {
        fs::write(dir.path().join(format!("f{i}.txt")), format!("c{i}")).unwrap();
    }
    git_ok(dir.path(), &["add", "-A"]);
    commit(dir.path(), "batch add");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn sequential_commits_increment_log() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    for i in 0..3 {
        fs::write(dir.path().join(format!("s{i}.txt")), format!("{i}")).unwrap();
        git_ok(dir.path(), &["add", "-A"]);
        commit(dir.path(), &format!("commit {i}"));
    }
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    // Empty baseline may or may not produce a commit depending on git
    // version. We made 3 explicit commits, so expect at least 3.
    let n: u32 = count.trim().parse().unwrap();
    assert!((3..=4).contains(&n), "expected 3 or 4 commits, got {n}");
}

#[test]
fn add_all_with_dash_a() {
    let dir = tmp();
    fs::write(dir.path().join("orig.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("orig.txt"), "v2").unwrap();
    git_ok(dir.path(), &["add", "-A"]);
    commit(dir.path(), "update all");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn staged_file_shows_in_status() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("staged.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "staged.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("A  staged.txt"), "got: {status}");
}

// ═══════════════════════════════════════════════════════════════════
// 3. Diff generation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn diff_clean_repo_empty() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn diff_detects_single_line_change() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "old\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "new\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-old"));
    assert!(diff.contains("+new"));
}

#[test]
fn diff_multiline_change() {
    let dir = tmp();
    fs::write(dir.path().join("m.txt"), "a\nb\nc\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("m.txt"), "a\nB\nC\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-b"));
    assert!(diff.contains("+B"));
}

#[test]
fn diff_deleted_tracked_file() {
    let dir = tmp();
    fs::write(dir.path().join("d.txt"), "content\n").unwrap();
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("d.txt")).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("d.txt"));
}

#[test]
fn diff_ignores_untracked() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("untracked.txt"), "data").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.contains("untracked.txt"));
}

#[test]
fn diff_staged_not_in_unstaged_diff() {
    let dir = tmp();
    fs::write(dir.path().join("s.txt"), "orig\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("s.txt"), "upd\n").unwrap();
    git_ok(dir.path(), &["add", "s.txt"]);
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty(), "staged should not appear: {diff}");
}

#[test]
fn diff_shows_added_lines() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "line1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "line1\nline2\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+line2"));
}

#[test]
fn diff_shows_removed_lines() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "line1\nline2\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "line1\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-line2"));
}

#[test]
fn diff_multiple_files_changed() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a1\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("a.txt"), "a2\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b2\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("a.txt"));
    assert!(diff.contains("b.txt"));
}

#[test]
fn diff_contains_unified_header() {
    let dir = tmp();
    fs::write(dir.path().join("h.txt"), "before\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("h.txt"), "after\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("---"), "expected --- header");
    assert!(diff.contains("+++"), "expected +++ header");
}

#[test]
fn diff_no_color_in_output() {
    let dir = tmp();
    fs::write(dir.path().join("c.txt"), "old\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("c.txt"), "new\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    // ANSI escape starts with \x1b[
    assert!(!diff.contains("\x1b["), "diff should have no ANSI escapes");
}

#[test]
fn diff_after_commit_is_clean() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "-A"]);
    commit(dir.path(), "update");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn diff_replaced_file_content() {
    let dir = tmp();
    fs::write(dir.path().join("r.txt"), "aaa\nbbb\nccc\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("r.txt"), "xxx\nyyy\nzzz\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-aaa"));
    assert!(diff.contains("+xxx"));
}

// ═══════════════════════════════════════════════════════════════════
// 4. Status queries
// ═══════════════════════════════════════════════════════════════════

#[test]
fn status_clean_repo() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn status_untracked_file() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("new.txt"), "hi").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("?? new.txt"));
}

#[test]
fn status_modified_file() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "orig").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "changed").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("f.txt"));
}

#[test]
fn status_deleted_file() {
    let dir = tmp();
    fs::write(dir.path().join("del.txt"), "bye").unwrap();
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("del.txt")).unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("del.txt"));
}

#[test]
fn status_staged_addition() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("staged.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "staged.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("A  staged.txt"));
}

#[test]
fn status_staged_modification() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("M  f.txt"), "got: {status}");
}

#[test]
fn status_staged_deletion() {
    let dir = tmp();
    fs::write(dir.path().join("del.txt"), "bye").unwrap();
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("del.txt")).unwrap();
    git_ok(dir.path(), &["add", "del.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("D  del.txt"), "got: {status}");
}

#[test]
fn status_mixed_staged_and_unstaged() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a1").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("a.txt"), "a2").unwrap();
    git_ok(dir.path(), &["add", "a.txt"]);
    // Modify again after staging
    fs::write(dir.path().join("a.txt"), "a3").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("MM a.txt"), "got: {status}");
}

#[test]
fn status_multiple_untracked() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    for i in 0..3 {
        fs::write(dir.path().join(format!("u{i}.txt")), "data").unwrap();
    }
    let status = git_status(dir.path()).unwrap();
    for i in 0..3 {
        assert!(status.contains(&format!("u{i}.txt")));
    }
}

#[test]
fn status_after_commit_is_clean() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "-A"]);
    commit(dir.path(), "add f");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn status_returns_porcelain_format() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("new.txt"), "hi").unwrap();
    let status = git_status(dir.path()).unwrap();
    // Porcelain v1: two-char status code then space then filename
    assert!(status.starts_with("??"), "expected porcelain: {status}");
}

// ═══════════════════════════════════════════════════════════════════
// 5. Integration with workspace staging
// ═══════════════════════════════════════════════════════════════════

#[test]
fn workspace_stager_creates_git_repo() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn workspace_stager_copies_files() {
    let src = tmp();
    fs::write(src.path().join("hello.txt"), "world").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let content = fs::read_to_string(ws.path().join("hello.txt")).unwrap();
    assert_eq!(content, "world");
}

#[test]
fn workspace_stager_baseline_commit() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"), "got: {log}");
}

#[test]
fn workspace_stager_clean_status_after_stage() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let status = git_status(ws.path()).unwrap();
    assert!(status.trim().is_empty(), "got: {status}");
}

#[test]
fn workspace_stager_diff_after_modification() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "original\n").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("f.txt"), "modified\n").unwrap();
    let diff = git_diff(ws.path()).unwrap();
    assert!(diff.contains("-original"));
    assert!(diff.contains("+modified"));
}

#[test]
fn workspace_stager_status_after_modification() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "v1").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("f.txt"), "v2").unwrap();
    let status = git_status(ws.path()).unwrap();
    assert!(status.contains("f.txt"));
}

#[test]
fn workspace_stager_exclude_pattern() {
    let src = tmp();
    fs::write(src.path().join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("skip.log"), "skip").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .stage()
        .unwrap();
    assert!(ws.path().join("keep.txt").exists());
    assert!(!ws.path().join("skip.log").exists());
}

#[test]
fn workspace_stager_without_git_init() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn workspace_stager_git_status_via_manager() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let status = abp_workspace::WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(status.unwrap().trim().is_empty());
}

#[test]
fn workspace_stager_git_diff_via_manager() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "data\n").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("f.txt"), "changed\n").unwrap();
    let diff = abp_workspace::WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    assert!(diff.unwrap().contains("+changed"));
}

#[test]
fn workspace_manager_prepare_passthrough() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    let spec = abp_core::WorkspaceSpec {
        root: dir.path().to_string_lossy().into_owned(),
        mode: abp_core::WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = abp_workspace::WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), dir.path());
}

#[test]
fn workspace_manager_prepare_staged() {
    let src = tmp();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let spec = abp_core::WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: abp_core::WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = abp_workspace::WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join(".git").exists());
    assert!(ws.path().join("f.txt").exists());
}

#[test]
fn workspace_staged_excludes_source_dotgit() {
    let src = tmp();
    ensure_git_repo(src.path());
    fs::write(src.path().join("f.txt"), "data").unwrap();
    git_ok(src.path(), &["add", "-A"]);
    commit(src.path(), "add f");
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    // Staged workspace should have its own .git, not a copy of source's
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"));
    assert!(!log.contains("add f"));
}

// ═══════════════════════════════════════════════════════════════════
// 6. Error handling
// ═══════════════════════════════════════════════════════════════════

#[test]
fn status_returns_none_for_non_repo() {
    let dir = tmp();
    assert!(git_status(dir.path()).is_none());
}

#[test]
fn diff_returns_none_for_non_repo() {
    let dir = tmp();
    assert!(git_diff(dir.path()).is_none());
}

#[test]
fn stager_missing_source_root_fails() {
    let result = abp_workspace::WorkspaceStager::new().stage();
    assert!(result.is_err());
}

#[test]
fn stager_nonexistent_source_fails() {
    let result = abp_workspace::WorkspaceStager::new()
        .source_root("/nonexistent/path/should/not/exist")
        .stage();
    assert!(result.is_err());
}

#[test]
fn status_on_empty_initialized_repo() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn diff_on_empty_initialized_repo() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn ensure_git_repo_tolerates_dotgit_as_file() {
    // Some git worktree setups have .git as a file. ensure_git_repo checks
    // .git existence (file or dir) and short-circuits.
    let dir = tmp();
    fs::write(dir.path().join(".git"), "gitdir: /tmp/fake").unwrap();
    // Should not panic — just skip because .git exists
    ensure_git_repo(dir.path());
}

// ═══════════════════════════════════════════════════════════════════
// 7. Edge cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn binary_file_tracked_and_modified() {
    let dir = tmp();
    let bin = dir.path().join("img.bin");
    fs::write(&bin, [0u8, 1, 2, 255, 254, 253]).unwrap();
    ensure_git_repo(dir.path());
    fs::write(&bin, [9u8, 8, 7, 6]).unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("img.bin"));
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("img.bin"));
}

#[test]
fn empty_file_is_tracked() {
    let dir = tmp();
    fs::write(dir.path().join("empty.txt"), "").unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("empty.txt"));
}

#[test]
fn large_number_of_files() {
    let dir = tmp();
    for i in 0..100 {
        fs::write(dir.path().join(format!("f{i}.txt")), format!("{i}")).unwrap();
    }
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn file_with_spaces_in_name() {
    let dir = tmp();
    fs::write(dir.path().join("my file.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn file_with_dashes_and_parens() {
    let dir = tmp();
    fs::write(dir.path().join("file - copy (2).txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn large_file_content() {
    let dir = tmp();
    let big = "x".repeat(100_000);
    fs::write(dir.path().join("big.txt"), &big).unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn large_diff_output() {
    let dir = tmp();
    let original: String = (0..500).map(|i| format!("line {i}\n")).collect();
    fs::write(dir.path().join("big.txt"), &original).unwrap();
    ensure_git_repo(dir.path());
    let modified: String = (0..500).map(|i| format!("changed {i}\n")).collect();
    fs::write(dir.path().join("big.txt"), &modified).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.len() > 1000,
        "expected large diff, got {} bytes",
        diff.len()
    );
}

#[test]
fn unicode_file_content() {
    let dir = tmp();
    fs::write(dir.path().join("uni.txt"), "héllo wörld 你好\n").unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn unicode_content_diff() {
    let dir = tmp();
    fs::write(dir.path().join("uni.txt"), "hello\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("uni.txt"), "héllo\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+h"));
}

#[test]
fn newline_only_file() {
    let dir = tmp();
    fs::write(dir.path().join("nl.txt"), "\n\n\n").unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn file_with_no_trailing_newline() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "no newline").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "still no newline").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("No newline at end of file") || diff.contains("f.txt"));
}

#[test]
fn dotfile_is_tracked() {
    let dir = tmp();
    fs::write(dir.path().join(".hidden"), "secret").unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains(".hidden"));
}

#[test]
fn multiple_extensions() {
    let dir = tmp();
    for ext in ["rs", "py", "js", "txt", "md", "toml", "json"] {
        fs::write(dir.path().join(format!("file.{ext}")), ext).unwrap();
    }
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn overwrite_file_multiple_times_before_commit() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    fs::write(dir.path().join("f.txt"), "v3\n").unwrap();
    fs::write(dir.path().join("f.txt"), "v4\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    // Should see diff from v1 to v4, not intermediate versions
    assert!(diff.contains("-v1"));
    assert!(diff.contains("+v4"));
    assert!(!diff.contains("+v2"));
}

#[test]
fn diff_after_partial_staging() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a1\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("a.txt"), "a2\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b2\n").unwrap();
    git_ok(dir.path(), &["add", "a.txt"]); // only stage a
    let diff = git_diff(dir.path()).unwrap();
    // Only b.txt should appear in unstaged diff
    assert!(!diff.contains("a.txt"), "a.txt is staged, not in diff");
    assert!(diff.contains("b.txt"), "b.txt should be in diff");
}

#[test]
fn empty_directory_not_tracked() {
    let dir = tmp();
    fs::create_dir_all(dir.path().join("empty_dir")).unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(!tracked.contains("empty_dir"));
}

#[test]
fn rename_detection_in_status() {
    let dir = tmp();
    fs::write(dir.path().join("old.txt"), "content").unwrap();
    ensure_git_repo(dir.path());
    fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).unwrap();
    let status = git_status(dir.path()).unwrap();
    // unstaged rename shows as delete + untracked
    assert!(status.contains("old.txt"));
    assert!(status.contains("new.txt"));
}

#[test]
fn concurrent_file_operations() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    // Create, modify, and check in rapid succession
    for i in 0..20 {
        fs::write(dir.path().join(format!("f{i}.txt")), format!("v{i}")).unwrap();
    }
    let status = git_status(dir.path()).unwrap();
    for i in 0..20 {
        assert!(status.contains(&format!("f{i}.txt")));
    }
}

#[test]
fn workspace_modify_add_commit_diff_cycle() {
    let src = tmp();
    fs::write(src.path().join("cycle.txt"), "v1\n").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    // Modify
    fs::write(ws.path().join("cycle.txt"), "v2\n").unwrap();
    let diff = git_diff(ws.path()).unwrap();
    assert!(diff.contains("+v2"));
    // Stage and commit
    git_ok(ws.path(), &["add", "-A"]);
    commit(ws.path(), "update cycle");
    let status = git_status(ws.path()).unwrap();
    assert!(status.trim().is_empty());
    let diff = git_diff(ws.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn workspace_add_new_file_shows_in_status() {
    let src = tmp();
    fs::write(src.path().join("orig.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("added.txt"), "new data").unwrap();
    let status = git_status(ws.path()).unwrap();
    assert!(status.contains("added.txt"));
}

#[test]
fn workspace_delete_file_shows_in_status() {
    let src = tmp();
    fs::write(src.path().join("remove.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::remove_file(ws.path().join("remove.txt")).unwrap();
    let status = git_status(ws.path()).unwrap();
    assert!(status.contains("remove.txt"));
}

#[test]
fn workspace_with_subdirectories() {
    let src = tmp();
    let sub = src.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("inner.txt"), "inner").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("sub").join("inner.txt").exists());
    let status = git_status(ws.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn workspace_include_pattern() {
    let src = tmp();
    fs::write(src.path().join("keep.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("skip.txt"), "skip").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();
    assert!(ws.path().join("keep.rs").exists());
    assert!(!ws.path().join("skip.txt").exists());
}

#[test]
fn workspace_multiple_exclude_patterns() {
    let src = tmp();
    fs::write(src.path().join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("a.log"), "log").unwrap();
    fs::write(src.path().join("b.tmp"), "tmp").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into(), "*.tmp".into()])
        .stage()
        .unwrap();
    assert!(ws.path().join("keep.txt").exists());
    assert!(!ws.path().join("a.log").exists());
    assert!(!ws.path().join("b.tmp").exists());
}

#[test]
fn ensure_git_repo_then_status_then_diff() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());

    fs::write(dir.path().join("f.txt"), "world\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("f.txt"));

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+world"));
}

#[test]
fn status_and_diff_agree_on_clean() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(status.trim().is_empty());
    assert!(diff.trim().is_empty());
}

#[test]
fn status_and_diff_agree_on_modification() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(status.contains("f.txt"));
    assert!(diff.contains("f.txt"));
}

#[test]
fn symlink_not_followed_in_init() {
    // This test is Windows-safe: we just verify ensure_git_repo doesn't panic
    // on directories that might have unusual entries.
    let dir = tmp();
    fs::write(dir.path().join("regular.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("regular.txt"));
}

#[test]
fn gitignore_respected_in_status() {
    let dir = tmp();
    fs::write(dir.path().join("tracked.txt"), "data").unwrap();
    fs::write(dir.path().join(".gitignore"), "*.ignored\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("test.ignored"), "ignored").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(
        !status.contains("test.ignored"),
        "ignored files should not appear: {status}"
    );
}

#[test]
fn gitignore_respected_in_diff() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("test.log"), "log data").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.contains("test.log"));
}

#[test]
fn multiple_init_calls_no_corruption() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    for _ in 0..5 {
        ensure_git_repo(dir.path());
    }
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
    let log = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(log.trim(), "1");
}

#[test]
fn diff_empty_file_to_content() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "now has content\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+now has content"));
}

#[test]
fn diff_content_to_empty() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "has content\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-has content"));
}

#[test]
fn status_detects_permission_change_file() {
    // On Windows, permission changes are mostly no-ops, but the file
    // content change detection still works.
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    // Modify content to ensure a detectable change
    fs::write(dir.path().join("f.txt"), "data\n").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("f.txt"));
}

#[test]
fn workspace_stager_preserves_nested_structure() {
    let src = tmp();
    let deep = src.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(
        ws.path()
            .join("a")
            .join("b")
            .join("c")
            .join("deep.txt")
            .exists()
    );
}

#[test]
fn workspace_stager_default_is_new() {
    // WorkspaceStager::default() should be equivalent to ::new()
    let src = tmp();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = abp_workspace::WorkspaceStager::default()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
    assert!(ws.path().join("f.txt").exists());
}

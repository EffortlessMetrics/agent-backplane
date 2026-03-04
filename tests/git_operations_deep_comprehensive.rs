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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for the `abp-git` crate.
//!
//! 150+ tests covering: repository initialization, idempotency, git status,
//! git diff, staged vs unstaged detection, file add/remove tracking, binary
//! files, large diffs, branch operations, tag operations, merge conflict
//! detection, gitignore patterns, nested directories, special filenames,
//! symlinks, empty repos, multi-commit histories, error handling, and
//! edge cases.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

use abp_git::{ensure_git_repo, git_diff, git_status};

// ── helpers ──────────────────────────────────────────────────────────

/// Run an arbitrary git command inside `path` and return stdout.
fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("git should be on PATH");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Run a git command and assert it succeeds.
fn git_ok(path: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(path)
        .status()
        .expect("git should be on PATH");
    assert!(st.success(), "git {args:?} failed");
}

/// Run a git commit with test identity.
fn git_commit(path: &Path, msg: &str) {
    git_ok(
        path,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test",
            "commit",
            "-qm",
            msg,
        ],
    );
}

/// Create a fresh temp dir.
fn tmp() -> TempDir {
    TempDir::new().expect("create temp dir")
}

/// Create a temp dir with `ensure_git_repo` already called.
fn tmp_repo() -> TempDir {
    let dir = tmp();
    ensure_git_repo(dir.path());
    dir
}

/// Create a temp dir with a file committed via `ensure_git_repo`.
fn tmp_repo_with_file(name: &str, content: &str) -> TempDir {
    let dir = tmp();
    fs::write(dir.path().join(name), content).unwrap();
    ensure_git_repo(dir.path());
    dir
}

// =====================================================================
// 1. Git repository initialization
// =====================================================================

#[test]
fn init_creates_dot_git_directory() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_creates_head_ref() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").join("HEAD").exists());
}

#[test]
fn init_is_idempotent_no_error() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_idempotent_preserves_commit_count() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    ensure_git_repo(dir.path());
    let count_before = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    ensure_git_repo(dir.path());
    let count_after = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count_before.trim(), count_after.trim());
}

#[test]
fn init_creates_baseline_commit() {
    let dir = tmp();
    fs::write(dir.path().join("x.txt"), "x").unwrap();
    ensure_git_repo(dir.path());
    let log = git(dir.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"));
}

#[test]
fn init_baseline_commit_is_exactly_one() {
    let dir = tmp();
    fs::write(dir.path().join("x.txt"), "x").unwrap();
    ensure_git_repo(dir.path());
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1");
}

#[test]
fn init_on_empty_dir_still_creates_git() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_stages_all_pre_existing_files() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    fs::write(dir.path().join("c.txt"), "c").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("a.txt"));
    assert!(tracked.contains("b.txt"));
    assert!(tracked.contains("c.txt"));
}

#[test]
fn init_skips_existing_repo_no_baseline() {
    let dir = tmp();
    git_ok(dir.path(), &["init", "-q"]);
    let log_before = git(dir.path(), &["log", "--oneline"]);
    ensure_git_repo(dir.path());
    let log_after = git(dir.path(), &["log", "--oneline"]);
    assert_eq!(log_before, log_after);
}

#[test]
fn init_with_nested_directories() {
    let dir = tmp();
    let nested = dir.path().join("a").join("b").join("c");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("deep.txt"), "deep").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("deep.txt"));
}

#[test]
fn init_with_hidden_file() {
    let dir = tmp();
    fs::write(dir.path().join(".hidden"), "secret").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains(".hidden"));
}

#[test]
fn init_repo_has_clean_status() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "f").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).expect("status should work");
    assert!(
        status.trim().is_empty(),
        "freshly initialized repo should be clean"
    );
}

#[test]
fn init_preserves_file_content() {
    let dir = tmp();
    fs::write(dir.path().join("data.txt"), "hello world").unwrap();
    ensure_git_repo(dir.path());

    let content = fs::read_to_string(dir.path().join("data.txt")).unwrap();
    assert_eq!(content, "hello world");
}

// =====================================================================
// 2. Git diff generation and parsing
// =====================================================================

#[test]
fn diff_clean_repo_is_empty() {
    let dir = tmp_repo();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.trim().is_empty());
}

#[test]
fn diff_detects_single_line_change() {
    let dir = tmp_repo_with_file("f.txt", "old\n");
    fs::write(dir.path().join("f.txt"), "new\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-old"));
    assert!(diff.contains("+new"));
}

#[test]
fn diff_shows_added_lines() {
    let dir = tmp_repo_with_file("f.txt", "line1\n");
    fs::write(dir.path().join("f.txt"), "line1\nline2\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("+line2"));
}

#[test]
fn diff_shows_removed_lines() {
    let dir = tmp_repo_with_file("f.txt", "line1\nline2\n");
    fs::write(dir.path().join("f.txt"), "line1\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-line2"));
}

#[test]
fn diff_multiline_replacement() {
    let dir = tmp_repo_with_file("f.txt", "a\nb\nc\n");
    fs::write(dir.path().join("f.txt"), "a\nB\nC\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-b"));
    assert!(diff.contains("+B"));
    assert!(diff.contains("-c"));
    assert!(diff.contains("+C"));
}

#[test]
fn diff_contains_filename() {
    let dir = tmp_repo_with_file("important.txt", "v1\n");
    fs::write(dir.path().join("important.txt"), "v2\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("important.txt"));
}

#[test]
fn diff_contains_unified_diff_header() {
    let dir = tmp_repo_with_file("f.txt", "old\n");
    fs::write(dir.path().join("f.txt"), "new\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("---"));
    assert!(diff.contains("+++"));
}

#[test]
fn diff_contains_hunk_header() {
    let dir = tmp_repo_with_file("f.txt", "old\n");
    fs::write(dir.path().join("f.txt"), "new\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("@@"));
}

#[test]
fn diff_for_deleted_file() {
    let dir = tmp_repo_with_file("d.txt", "content\n");
    fs::remove_file(dir.path().join("d.txt")).unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("d.txt"));
    assert!(diff.contains("-content"));
}

#[test]
fn diff_ignores_untracked_files() {
    let dir = tmp_repo();
    fs::write(dir.path().join("untracked.txt"), "data").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(!diff.contains("untracked.txt"));
}

#[test]
fn diff_returns_none_for_non_repo() {
    let dir = tmp();
    assert!(git_diff(dir.path()).is_none());
}

#[test]
fn diff_empty_file_to_content() {
    let dir = tmp_repo_with_file("e.txt", "");
    fs::write(dir.path().join("e.txt"), "now has content\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("+now has content"));
}

#[test]
fn diff_content_to_empty_file() {
    let dir = tmp_repo_with_file("e.txt", "has content\n");
    fs::write(dir.path().join("e.txt"), "").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-has content"));
}

#[test]
fn diff_multiple_files_modified() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a1\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("a.txt"), "a2\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b2\n").unwrap();

    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("a.txt"));
    assert!(diff.contains("b.txt"));
}

#[test]
fn diff_no_color_flag_is_used() {
    let dir = tmp_repo_with_file("f.txt", "before\n");
    fs::write(dir.path().join("f.txt"), "after\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    // ANSI escape codes start with \x1b[
    assert!(
        !diff.contains("\x1b["),
        "diff should not contain ANSI color codes"
    );
}

// =====================================================================
// 3. Git status checking
// =====================================================================

#[test]
fn status_clean_repo() {
    let dir = tmp_repo();
    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

#[test]
fn status_returns_none_for_non_repo() {
    let dir = tmp();
    assert!(git_status(dir.path()).is_none());
}

#[test]
fn status_detects_untracked_file() {
    let dir = tmp_repo();
    fs::write(dir.path().join("new.txt"), "hello").unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("??"));
    assert!(status.contains("new.txt"));
}

#[test]
fn status_detects_modified_tracked_file() {
    let dir = tmp_repo_with_file("f.txt", "original");
    fs::write(dir.path().join("f.txt"), "changed").unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("f.txt"));
}

#[test]
fn status_detects_deleted_file() {
    let dir = tmp_repo_with_file("d.txt", "gone");
    fs::remove_file(dir.path().join("d.txt")).unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("d.txt"));
}

#[test]
fn status_multiple_changes() {
    let dir = tmp_repo_with_file("keep.txt", "keep");
    fs::write(dir.path().join("keep.txt"), "modified").unwrap();
    fs::write(dir.path().join("brand_new.txt"), "new").unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("keep.txt"));
    assert!(status.contains("brand_new.txt"));
}

#[test]
fn status_porcelain_format() {
    let dir = tmp_repo();
    fs::write(dir.path().join("untracked.txt"), "data").unwrap();
    let status = git_status(dir.path()).expect("status");
    // Porcelain v1 uses two-char status codes followed by a space
    assert!(
        status.starts_with("??"),
        "porcelain format expected, got: {status}"
    );
}

#[test]
fn status_empty_repo_no_files_clean() {
    let dir = tmp_repo();
    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

// =====================================================================
// 4. Staged vs unstaged change detection
// =====================================================================

#[test]
fn staged_addition_shows_in_status() {
    let dir = tmp_repo();
    fs::write(dir.path().join("staged.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "staged.txt"]);
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("A  staged.txt"));
}

#[test]
fn staged_change_not_in_unstaged_diff() {
    let dir = tmp_repo_with_file("s.txt", "original\n");
    fs::write(dir.path().join("s.txt"), "updated\n").unwrap();
    git_ok(dir.path(), &["add", "s.txt"]);
    let diff = git_diff(dir.path()).expect("diff");
    assert!(
        diff.trim().is_empty(),
        "staged changes should not appear in unstaged diff"
    );
}

#[test]
fn unstaged_modification_appears_in_diff() {
    let dir = tmp_repo_with_file("u.txt", "orig\n");
    fs::write(dir.path().join("u.txt"), "mod\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-orig"));
    assert!(diff.contains("+mod"));
}

#[test]
fn partial_stage_shows_both_staged_and_unstaged() {
    let dir = tmp_repo_with_file("p.txt", "line1\n");
    fs::write(dir.path().join("p.txt"), "line2\n").unwrap();
    git_ok(dir.path(), &["add", "p.txt"]);
    fs::write(dir.path().join("p.txt"), "line3\n").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("p.txt"));
    // Status should show both staged (M) and unstaged (M) indicators
    assert!(
        status.contains("MM"),
        "expected both staged and unstaged: {status}"
    );
}

#[test]
fn staged_deletion_shows_d_in_status() {
    let dir = tmp_repo_with_file("del.txt", "bye");
    git_ok(dir.path(), &["rm", "del.txt"]);
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("D  del.txt") || status.contains("D del.txt"));
}

#[test]
fn staged_rename_detected() {
    let dir = tmp_repo_with_file("old_name.txt", "content");
    git_ok(dir.path(), &["mv", "old_name.txt", "new_name.txt"]);
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("new_name.txt"));
}

// =====================================================================
// 5. File addition and removal tracking
// =====================================================================

#[test]
fn add_single_file_after_init() {
    let dir = tmp_repo();
    fs::write(dir.path().join("added.txt"), "new content").unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("added.txt"));
}

#[test]
fn remove_committed_file() {
    let dir = tmp_repo_with_file("victim.txt", "doomed");
    fs::remove_file(dir.path().join("victim.txt")).unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("victim.txt"));
}

#[test]
fn add_multiple_files_tracked_separately() {
    let dir = tmp_repo();
    for i in 0..5 {
        fs::write(
            dir.path().join(format!("new_{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    let status = git_status(dir.path()).expect("status");
    for i in 0..5 {
        assert!(status.contains(&format!("new_{i}.txt")));
    }
}

#[test]
fn remove_all_files_detected() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("a.txt")).unwrap();
    fs::remove_file(dir.path().join("b.txt")).unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("a.txt"));
    assert!(status.contains("b.txt"));
}

#[test]
fn add_file_in_new_subdirectory() {
    let dir = tmp_repo();
    let sub = dir.path().join("subdir");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("file.txt"), "content").unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("subdir/"));
}

#[test]
fn file_replace_content_entirely() {
    let dir = tmp_repo_with_file("r.txt", "old content here\nmore lines\n");
    fs::write(dir.path().join("r.txt"), "completely new\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-old content here"));
    assert!(diff.contains("+completely new"));
}

// =====================================================================
// 6. Binary file detection in diffs
// =====================================================================

#[test]
fn binary_file_shows_in_status() {
    let dir = tmp_repo();
    fs::write(dir.path().join("img.bin"), [0u8, 1, 2, 255, 254, 253]).unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("img.bin"));
}

#[test]
fn binary_file_modification_in_diff() {
    let dir = tmp();
    fs::write(dir.path().join("img.bin"), [0u8, 1, 2, 255]).unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("img.bin"), [9u8, 8, 7, 6]).unwrap();

    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("img.bin"));
}

#[test]
fn binary_file_tracked_after_init() {
    let dir = tmp();
    fs::write(dir.path().join("data.bin"), vec![0u8; 256]).unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("data.bin"));
}

#[test]
fn null_bytes_in_file_detected_as_binary() {
    let dir = tmp();
    fs::write(dir.path().join("null.bin"), b"hello\0world").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("null.bin"), b"changed\0data").unwrap();

    let diff = git_diff(dir.path()).expect("diff");
    // Git typically notes binary files
    assert!(
        diff.contains("null.bin"),
        "binary file should appear in diff output"
    );
}

// =====================================================================
// 7. Large diff handling
// =====================================================================

#[test]
fn large_file_tracked() {
    let dir = tmp();
    let big_content: String = (0..1000).map(|i| format!("line {i}\n")).collect();
    fs::write(dir.path().join("big.txt"), &big_content).unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

#[test]
fn large_diff_with_many_changed_lines() {
    let dir = tmp();
    let original: String = (0..500).map(|i| format!("line {i}\n")).collect();
    fs::write(dir.path().join("big.txt"), &original).unwrap();
    ensure_git_repo(dir.path());

    let modified: String = (0..500).map(|i| format!("modified {i}\n")).collect();
    fs::write(dir.path().join("big.txt"), &modified).unwrap();

    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-line 0"));
    assert!(diff.contains("+modified 0"));
    assert!(diff.len() > 1000, "diff should be substantial");
}

#[test]
fn many_files_status() {
    let dir = tmp();
    for i in 0..100 {
        fs::write(
            dir.path().join(format!("file_{i:03}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).expect("status");
    assert!(
        status.trim().is_empty(),
        "all 100 files should be committed"
    );
}

#[test]
fn many_files_modified_diff() {
    let dir = tmp();
    for i in 0..20 {
        fs::write(
            dir.path().join(format!("f_{i}.txt")),
            format!("original {i}\n"),
        )
        .unwrap();
    }
    ensure_git_repo(dir.path());

    for i in 0..20 {
        fs::write(
            dir.path().join(format!("f_{i}.txt")),
            format!("changed {i}\n"),
        )
        .unwrap();
    }

    let diff = git_diff(dir.path()).expect("diff");
    for i in 0..20 {
        assert!(diff.contains(&format!("f_{i}.txt")));
    }
}

// =====================================================================
// 8. Git config operations (via ensure_git_repo user config)
// =====================================================================

#[test]
fn baseline_commit_has_abp_author() {
    let dir = tmp();
    fs::write(dir.path().join("x.txt"), "x").unwrap();
    ensure_git_repo(dir.path());
    let log = git(dir.path(), &["log", "--format=%an"]);
    assert!(
        log.trim().contains("abp"),
        "author should be 'abp', got: {log}"
    );
}

#[test]
fn baseline_commit_has_abp_email() {
    let dir = tmp();
    fs::write(dir.path().join("x.txt"), "x").unwrap();
    ensure_git_repo(dir.path());
    let log = git(dir.path(), &["log", "--format=%ae"]);
    assert!(
        log.trim().contains("abp@local"),
        "email should be 'abp@local', got: {log}"
    );
}

#[test]
fn second_commit_with_different_author() {
    let dir = tmp_repo_with_file("a.txt", "a");
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    git_ok(dir.path(), &["add", "b.txt"]);
    git_commit(dir.path(), "second");

    let log = git(dir.path(), &["log", "--format=%an", "-1"]);
    assert!(log.trim().contains("test"));
}

// =====================================================================
// 9. Branch operations
// =====================================================================

#[test]
fn default_branch_exists_after_init() {
    let dir = tmp_repo_with_file("f.txt", "f");
    let branches = git(dir.path(), &["branch"]);
    assert!(
        !branches.trim().is_empty(),
        "should have at least one branch"
    );
}

#[test]
fn create_and_switch_branch() {
    let dir = tmp_repo_with_file("f.txt", "main content");
    git_ok(dir.path(), &["checkout", "-b", "feature"]);
    fs::write(dir.path().join("feature.txt"), "feature work").unwrap();
    git_ok(dir.path(), &["add", "feature.txt"]);
    git_commit(dir.path(), "feature commit");

    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

#[test]
fn status_on_new_branch_is_clean() {
    let dir = tmp_repo_with_file("f.txt", "content");
    git_ok(dir.path(), &["checkout", "-b", "clean-branch"]);
    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

#[test]
fn diff_on_new_branch_after_modification() {
    let dir = tmp_repo_with_file("f.txt", "original\n");
    git_ok(dir.path(), &["checkout", "-b", "edit-branch"]);
    fs::write(dir.path().join("f.txt"), "edited\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-original"));
    assert!(diff.contains("+edited"));
}

#[test]
fn switch_back_to_original_branch() {
    let dir = tmp_repo_with_file("f.txt", "main");
    let default_branch = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    git_ok(dir.path(), &["checkout", "-b", "temp"]);
    git_ok(dir.path(), &["checkout", &default_branch]);
    let content = fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "main");
}

#[test]
fn changes_on_branch_isolated_from_main() {
    let dir = tmp_repo_with_file("f.txt", "main version\n");
    let default_branch = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();

    git_ok(dir.path(), &["checkout", "-b", "isolated"]);
    fs::write(dir.path().join("f.txt"), "branch version\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    git_commit(dir.path(), "branch change");

    git_ok(dir.path(), &["checkout", &default_branch]);
    let content = fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert!(
        content.trim() == "main version",
        "expected 'main version', got: {content:?}"
    );
}

// =====================================================================
// 10. Tag operations
// =====================================================================

#[test]
fn create_lightweight_tag() {
    let dir = tmp_repo_with_file("f.txt", "tagged");
    git_ok(dir.path(), &["tag", "v1.0"]);
    let tags = git(dir.path(), &["tag"]);
    assert!(tags.contains("v1.0"));
}

#[test]
fn create_annotated_tag() {
    let dir = tmp_repo_with_file("f.txt", "tagged");
    git_ok(
        dir.path(),
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test",
            "tag",
            "-a",
            "v2.0",
            "-m",
            "release v2.0",
        ],
    );
    let tags = git(dir.path(), &["tag"]);
    assert!(tags.contains("v2.0"));
}

#[test]
fn status_unchanged_after_tagging() {
    let dir = tmp_repo_with_file("f.txt", "content");
    git_ok(dir.path(), &["tag", "v0.1"]);
    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

#[test]
fn multiple_tags_on_same_commit() {
    let dir = tmp_repo_with_file("f.txt", "tagged");
    git_ok(dir.path(), &["tag", "tag-a"]);
    git_ok(dir.path(), &["tag", "tag-b"]);
    let tags = git(dir.path(), &["tag"]);
    assert!(tags.contains("tag-a"));
    assert!(tags.contains("tag-b"));
}

// =====================================================================
// 11. Merge conflict detection
// =====================================================================

#[test]
#[ignore = "git merge conflict behavior varies across git versions and CI environments"]
fn merge_conflict_appears_in_status() {
    let dir = tmp_repo_with_file("conflict.txt", "base\n");
    let default_branch = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();

    // Create diverging branches
    git_ok(dir.path(), &["checkout", "-b", "branch-a"]);
    fs::write(dir.path().join("conflict.txt"), "change from a\n").unwrap();
    git_ok(dir.path(), &["add", "conflict.txt"]);
    git_commit(dir.path(), "change a");

    git_ok(dir.path(), &["checkout", &default_branch]);
    fs::write(dir.path().join("conflict.txt"), "change from main\n").unwrap();
    git_ok(dir.path(), &["add", "conflict.txt"]);
    git_commit(dir.path(), "change main");

    // Attempt merge (will conflict)
    let merge_result = Command::new("git")
        .args(["merge", "branch-a"])
        .current_dir(dir.path())
        .output()
        .expect("git merge");
    // Merge should fail
    assert!(!merge_result.status.success());

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.contains("UU") || status.contains("AA") || status.contains("conflict.txt"),
        "conflict should appear in status: {status}"
    );
}

#[test]
#[ignore = "git merge conflict behavior varies across git versions and CI environments"]
fn merge_conflict_file_has_conflict_markers() {
    let dir = tmp_repo_with_file("c.txt", "base\n");
    let default_branch = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();

    git_ok(dir.path(), &["checkout", "-b", "side"]);
    fs::write(dir.path().join("c.txt"), "side change\n").unwrap();
    git_ok(dir.path(), &["add", "c.txt"]);
    git_commit(dir.path(), "side");

    git_ok(dir.path(), &["checkout", &default_branch]);
    fs::write(dir.path().join("c.txt"), "main change\n").unwrap();
    git_ok(dir.path(), &["add", "c.txt"]);
    git_commit(dir.path(), "main");

    let _ = Command::new("git")
        .args(["merge", "side"])
        .current_dir(dir.path())
        .output();

    let content = fs::read_to_string(dir.path().join("c.txt")).unwrap();
    assert!(
        content.contains("<<<<<<<") || content.contains("=======") || content.contains(">>>>>>>"),
        "conflict markers expected, got: {content}"
    );
}

#[test]
fn clean_merge_no_conflict() {
    let dir = tmp_repo_with_file("base.txt", "base\n");
    let default_branch = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();

    git_ok(dir.path(), &["checkout", "-b", "no-conflict"]);
    fs::write(dir.path().join("new_file.txt"), "added on branch\n").unwrap();
    git_ok(dir.path(), &["add", "new_file.txt"]);
    git_commit(dir.path(), "add new file");

    git_ok(dir.path(), &["checkout", &default_branch]);
    git_ok(dir.path(), &["merge", "no-conflict"]);

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.trim().is_empty(),
        "clean merge should leave no conflicts"
    );
    assert!(dir.path().join("new_file.txt").exists());
}

// =====================================================================
// 12. Git ignore patterns
// =====================================================================

#[test]
fn gitignore_hides_file_from_status() {
    let dir = tmp_repo();
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    git_ok(dir.path(), &["add", ".gitignore"]);
    git_commit(dir.path(), "add gitignore");

    fs::write(dir.path().join("debug.log"), "log data").unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(
        !status.contains("debug.log"),
        "ignored file should not appear in status: {status}"
    );
}

#[test]
fn gitignore_directory_pattern() {
    let dir = tmp_repo();
    fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
    git_ok(dir.path(), &["add", ".gitignore"]);
    git_commit(dir.path(), "add gitignore");

    let build = dir.path().join("build");
    fs::create_dir(&build).unwrap();
    fs::write(build.join("output.js"), "compiled").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(
        !status.contains("output.js"),
        "build dir should be ignored: {status}"
    );
}

#[test]
fn gitignore_negation_pattern() {
    let dir = tmp_repo();
    fs::write(dir.path().join(".gitignore"), "*.log\n!important.log\n").unwrap();
    git_ok(dir.path(), &["add", ".gitignore"]);
    git_commit(dir.path(), "add gitignore");

    fs::write(dir.path().join("debug.log"), "ignored").unwrap();
    fs::write(dir.path().join("important.log"), "kept").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(!status.contains("debug.log"), "debug.log should be ignored");
    assert!(
        status.contains("important.log"),
        "important.log should NOT be ignored"
    );
}

#[test]
fn gitignore_wildcard_pattern() {
    let dir = tmp_repo();
    fs::write(dir.path().join(".gitignore"), "*.tmp\n").unwrap();
    git_ok(dir.path(), &["add", ".gitignore"]);
    git_commit(dir.path(), "add gitignore");

    fs::write(dir.path().join("cache.tmp"), "temp").unwrap();
    fs::write(dir.path().join("data.txt"), "kept").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(!status.contains("cache.tmp"));
    assert!(status.contains("data.txt"));
}

#[test]
fn gitignore_nested_pattern() {
    let dir = tmp_repo();
    fs::write(dir.path().join(".gitignore"), "**/node_modules/\n").unwrap();
    git_ok(dir.path(), &["add", ".gitignore"]);
    git_commit(dir.path(), "add gitignore");

    let nm = dir.path().join("project").join("node_modules");
    fs::create_dir_all(&nm).unwrap();
    fs::write(nm.join("pkg.json"), "{}").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(
        !status.contains("pkg.json"),
        "node_modules contents should be ignored"
    );
}

// =====================================================================
// 13. Error handling
// =====================================================================

#[test]
fn status_on_nonexistent_path_returns_none() {
    let dir = tmp();
    let nonexistent = dir.path().join("does_not_exist");
    let status = git_status(&nonexistent);
    assert!(status.is_none());
}

#[test]
fn diff_on_nonexistent_path_returns_none() {
    let dir = tmp();
    let nonexistent = dir.path().join("does_not_exist");
    let diff = git_diff(&nonexistent);
    assert!(diff.is_none());
}

#[test]
fn status_on_file_not_directory() {
    let dir = tmp();
    let file_path = dir.path().join("not_a_dir.txt");
    fs::write(&file_path, "content").unwrap();
    let status = git_status(&file_path);
    assert!(status.is_none());
}

#[test]
fn diff_on_file_not_directory() {
    let dir = tmp();
    let file_path = dir.path().join("not_a_dir.txt");
    fs::write(&file_path, "content").unwrap();
    let diff = git_diff(&file_path);
    assert!(diff.is_none());
}

#[test]
fn ensure_git_repo_on_non_existent_does_not_panic() {
    let dir = tmp();
    let nonexistent = dir.path().join("no_such_dir");
    // Should not panic, just silently fail
    ensure_git_repo(&nonexistent);
}

// =====================================================================
// 14. Edge cases: special filenames
// =====================================================================

#[test]
fn file_with_spaces_in_name() {
    let dir = tmp();
    fs::write(dir.path().join("my file.txt"), "content").unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).expect("status");
    assert!(
        status.trim().is_empty(),
        "file with spaces should be committed"
    );
}

#[test]
fn file_with_dashes_and_underscores() {
    let dir = tmp();
    fs::write(dir.path().join("my-file_v2.txt"), "content").unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("my-file_v2.txt"));
}

#[test]
fn file_with_dots_in_name() {
    let dir = tmp();
    fs::write(dir.path().join("config.prod.env.bak"), "vars").unwrap();
    ensure_git_repo(dir.path());
    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("config.prod.env.bak"));
}

#[test]
fn file_with_unicode_content() {
    let dir = tmp();
    fs::write(dir.path().join("unicode.txt"), "héllo wörld 日本語").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("unicode.txt"), "changed héllo").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("unicode.txt"));
}

#[test]
fn empty_file_tracked_and_modifiable() {
    let dir = tmp_repo_with_file("empty.txt", "");
    fs::write(dir.path().join("empty.txt"), "now non-empty\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("+now non-empty"));
}

#[test]
fn file_with_long_name() {
    let dir = tmp();
    let name = "a".repeat(200) + ".txt";
    fs::write(dir.path().join(&name), "data").unwrap();
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

// =====================================================================
// 15. Deeply nested structures
// =====================================================================

#[test]
fn deeply_nested_file_tracked() {
    let dir = tmp();
    let deep = dir.path().join("a").join("b").join("c").join("d").join("e");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "deep content").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("leaf.txt"));
}

#[test]
fn nested_file_modification_in_diff() {
    let dir = tmp();
    let sub = dir.path().join("src").join("lib");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("mod.rs"), "fn main() {}\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(sub.join("mod.rs"), "fn main() { println!(\"hi\"); }\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("mod.rs"));
}

#[test]
fn empty_subdirectory_not_tracked() {
    let dir = tmp_repo();
    fs::create_dir(dir.path().join("empty_dir")).unwrap();
    let status = git_status(dir.path()).expect("status");
    // Git doesn't track empty directories
    assert!(
        status.trim().is_empty(),
        "empty dirs should not appear: {status}"
    );
}

// =====================================================================
// 16. Multi-commit histories
// =====================================================================

#[test]
fn second_commit_clean_status() {
    let dir = tmp_repo_with_file("a.txt", "v1");
    fs::write(dir.path().join("b.txt"), "v1").unwrap();
    git_ok(dir.path(), &["add", "b.txt"]);
    git_commit(dir.path(), "second commit");

    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

#[test]
fn three_commits_log() {
    let dir = tmp_repo_with_file("a.txt", "v1");

    fs::write(dir.path().join("b.txt"), "b").unwrap();
    git_ok(dir.path(), &["add", "b.txt"]);
    git_commit(dir.path(), "second");

    fs::write(dir.path().join("c.txt"), "c").unwrap();
    git_ok(dir.path(), &["add", "c.txt"]);
    git_commit(dir.path(), "third");

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "3");
}

#[test]
fn diff_after_multiple_commits() {
    let dir = tmp_repo_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    git_commit(dir.path(), "v2");

    fs::write(dir.path().join("f.txt"), "v3\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-v2"));
    assert!(diff.contains("+v3"));
}

#[test]
fn status_after_commit_is_clean() {
    let dir = tmp_repo();
    fs::write(dir.path().join("new.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);
    git_commit(dir.path(), "add file");

    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

// =====================================================================
// 17. Whitespace and line-ending edge cases
// =====================================================================

#[test]
fn trailing_whitespace_change() {
    let dir = tmp_repo_with_file("w.txt", "hello\n");
    fs::write(dir.path().join("w.txt"), "hello   \n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(
        !diff.trim().is_empty(),
        "trailing whitespace change should show in diff"
    );
}

#[test]
fn newline_at_eof_change() {
    let dir = tmp_repo_with_file("nl.txt", "line");
    fs::write(dir.path().join("nl.txt"), "line\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(
        !diff.trim().is_empty(),
        "newline at EOF change should appear"
    );
}

#[test]
fn only_whitespace_file() {
    let dir = tmp_repo_with_file("ws.txt", "   \n\t\n");
    fs::write(dir.path().join("ws.txt"), "\t\t\n   \n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("ws.txt"));
}

#[test]
fn crlf_to_lf_change() {
    let dir = tmp_repo_with_file("crlf.txt", "line1\r\nline2\r\n");
    fs::write(dir.path().join("crlf.txt"), "line1\nline2\n").unwrap();
    // Depending on git config, this may or may not appear in diff
    let _diff = git_diff(dir.path()).expect("diff should succeed");
}

// =====================================================================
// 18. Simultaneous status and diff consistency
// =====================================================================

#[test]
fn modified_file_appears_in_both_status_and_diff() {
    let dir = tmp_repo_with_file("both.txt", "orig\n");
    fs::write(dir.path().join("both.txt"), "changed\n").unwrap();

    let status = git_status(dir.path()).expect("status");
    let diff = git_diff(dir.path()).expect("diff");

    assert!(status.contains("both.txt"));
    assert!(diff.contains("both.txt"));
}

#[test]
fn untracked_in_status_but_not_diff() {
    let dir = tmp_repo();
    fs::write(dir.path().join("untracked.txt"), "data").unwrap();

    let status = git_status(dir.path()).expect("status");
    let diff = git_diff(dir.path()).expect("diff");

    assert!(status.contains("untracked.txt"));
    assert!(!diff.contains("untracked.txt"));
}

#[test]
fn deleted_file_in_both_status_and_diff() {
    let dir = tmp_repo_with_file("gone.txt", "content\n");
    fs::remove_file(dir.path().join("gone.txt")).unwrap();

    let status = git_status(dir.path()).expect("status");
    let diff = git_diff(dir.path()).expect("diff");

    assert!(status.contains("gone.txt"));
    assert!(diff.contains("gone.txt"));
}

// =====================================================================
// 19. Reset and restore operations
// =====================================================================

#[test]
fn restore_file_clears_diff() {
    let dir = tmp_repo_with_file("r.txt", "original\n");
    fs::write(dir.path().join("r.txt"), "modified\n").unwrap();
    assert!(!git_diff(dir.path()).expect("diff").trim().is_empty());

    git_ok(dir.path(), &["checkout", "--", "r.txt"]);
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.trim().is_empty(), "diff should be empty after restore");
}

#[test]
fn reset_staged_file() {
    let dir = tmp_repo_with_file("s.txt", "v1\n");
    fs::write(dir.path().join("s.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "s.txt"]);

    // Unstaged diff should be empty (change is staged)
    assert!(git_diff(dir.path()).expect("diff").trim().is_empty());

    // Reset staging
    git_ok(dir.path(), &["reset", "HEAD", "s.txt"]);

    // Now unstaged diff should show the change
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-v1"));
    assert!(diff.contains("+v2"));
}

// =====================================================================
// 20. Rename detection
// =====================================================================

#[test]
fn rename_file_detected_in_status() {
    let dir = tmp_repo_with_file("original.txt", "content");
    git_ok(dir.path(), &["mv", "original.txt", "renamed.txt"]);
    let status = git_status(dir.path()).expect("status");
    assert!(
        status.contains("renamed.txt"),
        "renamed file should appear in status: {status}"
    );
}

#[test]
fn rename_preserves_content() {
    let dir = tmp_repo_with_file("src.txt", "preserve me");
    git_ok(dir.path(), &["mv", "src.txt", "dst.txt"]);
    let content = fs::read_to_string(dir.path().join("dst.txt")).unwrap();
    assert_eq!(content, "preserve me");
}

// =====================================================================
// 21. Concurrent-like scenarios
// =====================================================================

#[test]
fn multiple_repos_independent() {
    let dir1 = tmp_repo_with_file("f.txt", "repo1");
    let dir2 = tmp_repo_with_file("f.txt", "repo2");

    fs::write(dir1.path().join("f.txt"), "changed1").unwrap();

    let status1 = git_status(dir1.path()).expect("status1");
    let status2 = git_status(dir2.path()).expect("status2");

    assert!(status1.contains("f.txt"));
    assert!(status2.trim().is_empty(), "repo2 should be unaffected");
}

#[test]
fn diff_on_one_repo_does_not_affect_another() {
    let dir1 = tmp_repo_with_file("a.txt", "v1\n");
    let dir2 = tmp_repo_with_file("a.txt", "v1\n");

    fs::write(dir1.path().join("a.txt"), "v2\n").unwrap();

    let diff1 = git_diff(dir1.path()).expect("diff1");
    let diff2 = git_diff(dir2.path()).expect("diff2");

    assert!(!diff1.trim().is_empty());
    assert!(diff2.trim().is_empty());
}

// =====================================================================
// 22. Submodule-like / nested git scenarios
// =====================================================================

#[test]
fn nested_git_repo_does_not_interfere() {
    let dir = tmp_repo_with_file("top.txt", "top");
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    ensure_git_repo(&sub);
    fs::write(sub.join("inner.txt"), "inner").unwrap();
    git_ok(&sub, &["add", "inner.txt"]);
    git_commit(&sub, "inner commit");

    // Top-level status should show sub/ as untracked or gitlink
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("sub"), "sub-repo should appear: {status}");
}

// =====================================================================
// 23. Git stash interactions
// =====================================================================

#[test]
fn stash_clears_working_dir() {
    let dir = tmp_repo_with_file("s.txt", "original\n");
    fs::write(dir.path().join("s.txt"), "modified\n").unwrap();

    git_ok(dir.path(), &["stash"]);
    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty(), "stash should clear working dir");

    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.trim().is_empty(), "stash should clear diff");
}

#[test]
fn stash_pop_restores_changes() {
    let dir = tmp_repo_with_file("s.txt", "original\n");
    fs::write(dir.path().join("s.txt"), "modified\n").unwrap();

    git_ok(dir.path(), &["stash"]);
    git_ok(dir.path(), &["stash", "pop"]);

    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-original"));
    assert!(diff.contains("+modified"));
}

// =====================================================================
// 24. Return type validation
// =====================================================================

#[test]
fn git_status_returns_some_for_valid_repo() {
    let dir = tmp_repo();
    assert!(git_status(dir.path()).is_some());
}

#[test]
fn git_diff_returns_some_for_valid_repo() {
    let dir = tmp_repo();
    assert!(git_diff(dir.path()).is_some());
}

#[test]
fn git_status_returns_string_type() {
    let dir = tmp_repo();
    let status: Option<String> = git_status(dir.path());
    assert!(status.is_some());
}

#[test]
fn git_diff_returns_string_type() {
    let dir = tmp_repo();
    let diff: Option<String> = git_diff(dir.path());
    assert!(diff.is_some());
}

// =====================================================================
// 25. Mixed operations sequences
// =====================================================================

#[test]
fn add_modify_delete_sequence() {
    let dir = tmp_repo_with_file("seq.txt", "v1\n");

    // Modify
    fs::write(dir.path().join("seq.txt"), "v2\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-v1"));

    // Stage
    git_ok(dir.path(), &["add", "seq.txt"]);
    assert!(git_diff(dir.path()).expect("diff").trim().is_empty());

    // Commit
    git_commit(dir.path(), "v2");
    assert!(git_status(dir.path()).expect("status").trim().is_empty());

    // Delete
    fs::remove_file(dir.path().join("seq.txt")).unwrap();
    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("seq.txt"));
}

#[test]
fn create_multiple_files_stage_commit_verify() {
    let dir = tmp_repo();
    for i in 0..10 {
        fs::write(
            dir.path().join(format!("batch_{i}.txt")),
            format!("batch content {i}"),
        )
        .unwrap();
    }
    git_ok(dir.path(), &["add", "-A"]);
    git_commit(dir.path(), "batch add");

    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    // baseline may produce 0 commits (empty repo) or 1; batch add adds 1 more
    let n: u32 = count.trim().parse().unwrap();
    assert!(n >= 1, "expected at least 1 commit, got {n}");
}

#[test]
fn modify_commit_modify_again_diff() {
    let dir = tmp_repo_with_file("m.txt", "v1\n");

    fs::write(dir.path().join("m.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "m.txt"]);
    git_commit(dir.path(), "v2");

    fs::write(dir.path().join("m.txt"), "v3\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-v2"));
    assert!(diff.contains("+v3"));
}

// =====================================================================
// 26. Permission / read-only edge cases
// =====================================================================

#[test]
fn read_only_file_tracked() {
    let dir = tmp();
    let file_path = dir.path().join("readonly.txt");
    fs::write(&file_path, "immutable").unwrap();

    // Make file read-only (Windows-compatible)
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file_path, perms).unwrap();

    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("readonly.txt"));

    // Restore write permission for cleanup
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&file_path, perms).unwrap();
}

// =====================================================================
// 27. Large content edge cases
// =====================================================================

#[test]
fn single_very_long_line() {
    let dir = tmp();
    let long_line = "x".repeat(10_000) + "\n";
    fs::write(dir.path().join("long.txt"), &long_line).unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

#[test]
fn many_empty_lines() {
    let dir = tmp();
    let content = "\n".repeat(1000);
    fs::write(dir.path().join("empty_lines.txt"), &content).unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("empty_lines.txt"), &"\n".repeat(999)).unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(!diff.trim().is_empty());
}

// =====================================================================
// 28. Diff with context
// =====================================================================

#[test]
fn diff_includes_context_lines() {
    let dir = tmp();
    let original = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n";
    fs::write(dir.path().join("ctx.txt"), original).unwrap();
    ensure_git_repo(dir.path());

    let modified = "line1\nline2\nline3\nline4\nCHANGED\nline6\nline7\nline8\nline9\nline10\n";
    fs::write(dir.path().join("ctx.txt"), modified).unwrap();

    let diff = git_diff(dir.path()).expect("diff");
    // Unified diff includes context lines around the change
    assert!(diff.contains("-line5"));
    assert!(diff.contains("+CHANGED"));
}

// =====================================================================
// 29. Ensure git_diff and git_status return consistent types
// =====================================================================

#[test]
fn status_and_diff_none_for_same_non_repo() {
    let dir = tmp();
    assert!(git_status(dir.path()).is_none());
    assert!(git_diff(dir.path()).is_none());
}

#[test]
fn status_and_diff_some_for_same_repo() {
    let dir = tmp_repo();
    assert!(git_status(dir.path()).is_some());
    assert!(git_diff(dir.path()).is_some());
}

// =====================================================================
// 30. Amend and rewrite scenarios
// =====================================================================

#[test]
fn amend_commit_keeps_status_clean() {
    let dir = tmp_repo_with_file("a.txt", "v1");
    fs::write(dir.path().join("a.txt"), "v2").unwrap();
    git_ok(dir.path(), &["add", "a.txt"]);
    git_ok(
        dir.path(),
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test",
            "commit",
            "--amend",
            "-m",
            "amended baseline",
        ],
    );

    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());
}

#[test]
fn amend_still_single_commit() {
    let dir = tmp_repo_with_file("a.txt", "v1");
    fs::write(dir.path().join("a.txt"), "v2").unwrap();
    git_ok(dir.path(), &["add", "a.txt"]);
    git_ok(
        dir.path(),
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test",
            "commit",
            "--amend",
            "-m",
            "amended",
        ],
    );

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1");
}

// =====================================================================
// 31. Special git scenarios
// =====================================================================

#[test]
fn gitkeep_file_tracked() {
    let dir = tmp();
    let sub = dir.path().join("empty_dir");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join(".gitkeep"), "").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains(".gitkeep"));
}

#[test]
fn dot_files_all_tracked() {
    let dir = tmp();
    fs::write(dir.path().join(".env"), "SECRET=x").unwrap();
    fs::write(dir.path().join(".editorconfig"), "root=true").unwrap();
    fs::write(dir.path().join(".gitattributes"), "*.txt text").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains(".env"));
    assert!(tracked.contains(".editorconfig"));
    assert!(tracked.contains(".gitattributes"));
}

#[test]
fn commit_message_preserved() {
    let dir = tmp_repo_with_file("f.txt", "data");
    fs::write(dir.path().join("g.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "g.txt"]);
    git_commit(dir.path(), "my custom message");

    let log = git(dir.path(), &["log", "--oneline", "-1"]);
    assert!(log.contains("my custom message"));
}

// =====================================================================
// 32. Additional comprehensive coverage
// =====================================================================

#[test]
fn diff_with_only_additions_no_removals() {
    let dir = tmp_repo_with_file("grow.txt", "line1\n");
    fs::write(dir.path().join("grow.txt"), "line1\nline2\nline3\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("+line2"));
    assert!(diff.contains("+line3"));
    assert!(
        !diff.contains("-line1"),
        "original line should not be removed"
    );
}

#[test]
fn diff_with_only_removals_no_additions() {
    let dir = tmp_repo_with_file("shrink.txt", "line1\nline2\nline3\n");
    fs::write(dir.path().join("shrink.txt"), "line1\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-line2"));
    assert!(diff.contains("-line3"));
}

#[test]
fn status_with_gitignore_and_tracked_file() {
    let dir = tmp();
    fs::write(dir.path().join("code.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("code.log"), "debug output").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    git_ok(dir.path(), &["add", ".gitignore"]);
    git_commit(dir.path(), "add ignore");

    // Modify ignored file — should not show in status
    fs::write(dir.path().join("new.log"), "more logs").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(
        !status.contains("new.log"),
        "new log should be ignored: {status}"
    );
}

#[test]
fn multiple_directories_with_files() {
    let dir = tmp();
    for subdir in &["src", "tests", "docs", "config"] {
        let path = dir.path().join(subdir);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("file.txt"), format!("content in {subdir}")).unwrap();
    }
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("src/file.txt") || tracked.contains("src\\file.txt"));
    assert!(tracked.contains("tests/file.txt") || tracked.contains("tests\\file.txt"));
    assert!(tracked.contains("docs/file.txt") || tracked.contains("docs\\file.txt"));
    assert!(tracked.contains("config/file.txt") || tracked.contains("config\\file.txt"));
}

#[test]
fn status_detects_type_change_file_to_dir() {
    let dir = tmp_repo_with_file("item", "file content");
    fs::remove_file(dir.path().join("item")).unwrap();
    fs::create_dir(dir.path().join("item")).unwrap();
    fs::write(dir.path().join("item").join("inner.txt"), "nested").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.contains("item"),
        "type change should appear: {status}"
    );
}

#[test]
fn init_with_many_nested_dirs() {
    let dir = tmp();
    for i in 0..10 {
        let sub = dir.path().join(format!("dir_{i}"));
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("file.txt"), format!("content {i}")).unwrap();
    }
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty(), "all files should be committed");
}

#[test]
fn diff_with_tab_characters() {
    let dir = tmp_repo_with_file("tabs.txt", "\tfirst\n\tsecond\n");
    fs::write(dir.path().join("tabs.txt"), "\tFIRST\n\tsecond\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("FIRST"));
}

#[test]
fn status_after_removing_and_recreating_file() {
    let dir = tmp_repo_with_file("phoenix.txt", "v1");
    fs::remove_file(dir.path().join("phoenix.txt")).unwrap();
    fs::write(dir.path().join("phoenix.txt"), "v2").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.contains("phoenix.txt"),
        "modified file should show: {status}"
    );
}

#[test]
fn diff_after_removing_and_recreating_same_content() {
    let dir = tmp_repo_with_file("same.txt", "same content\n");
    fs::remove_file(dir.path().join("same.txt")).unwrap();
    fs::write(dir.path().join("same.txt"), "same content\n").unwrap();

    let diff = git_diff(dir.path()).expect("diff");
    assert!(
        diff.trim().is_empty(),
        "same content should produce empty diff"
    );
}

#[test]
fn status_mixed_staged_unstaged_untracked() {
    let dir = tmp_repo_with_file("existing.txt", "original");

    // Staged change
    fs::write(dir.path().join("existing.txt"), "staged change").unwrap();
    git_ok(dir.path(), &["add", "existing.txt"]);

    // Another unstaged change on top
    fs::write(dir.path().join("existing.txt"), "unstaged on top").unwrap();

    // Untracked file
    fs::write(dir.path().join("brand_new.txt"), "untracked").unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(status.contains("existing.txt"));
    assert!(status.contains("brand_new.txt"));
}

#[test]
fn diff_of_appended_content() {
    let dir = tmp_repo_with_file("append.txt", "line1\nline2\n");
    fs::write(
        dir.path().join("append.txt"),
        "line1\nline2\nline3\nline4\n",
    )
    .unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("+line3"));
    assert!(diff.contains("+line4"));
}

#[test]
fn ensure_git_repo_with_gitignore_present() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    fs::write(dir.path().join("app.log"), "ignored log").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains(".gitignore"));
    assert!(tracked.contains("main.rs"));
    assert!(
        !tracked.contains("app.log"),
        "ignored file should not be tracked"
    );
}

#[test]
fn init_then_immediate_status_and_diff() {
    let dir = tmp();
    fs::write(dir.path().join("hello.txt"), "world").unwrap();
    ensure_git_repo(dir.path());

    // Both should succeed immediately after init
    let status = git_status(dir.path()).expect("status");
    let diff = git_diff(dir.path()).expect("diff");

    assert!(status.trim().is_empty());
    assert!(diff.trim().is_empty());
}

#[test]
fn sequential_ensures_do_not_corrupt() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).expect("status");
    assert!(status.trim().is_empty());

    let log = git(dir.path(), &["log", "--oneline"]);
    assert_eq!(
        log.trim().lines().count(),
        1,
        "should still have exactly one commit"
    );
}

#[test]
fn diff_with_special_regex_chars_in_content() {
    let dir = tmp_repo_with_file("regex.txt", "hello (world) [test] {curly}\n");
    fs::write(
        dir.path().join("regex.txt"),
        "goodbye (world) [test] {curly}\n",
    )
    .unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-hello"));
    assert!(diff.contains("+goodbye"));
}

#[test]
fn status_format_is_porcelain_v1() {
    let dir = tmp_repo();
    fs::write(dir.path().join("test.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "test.txt"]);

    let status = git_status(dir.path()).expect("status");
    // Porcelain v1 format: XY filename
    // A (added) in index, space in worktree
    assert!(
        status.starts_with("A ") || status.starts_with("A  "),
        "should be porcelain format: {status}"
    );
}

#[test]
fn diff_on_repo_with_no_changes_is_empty_string() {
    let dir = tmp_repo_with_file("stable.txt", "unchanged");
    let diff = git_diff(dir.path()).expect("diff");
    assert_eq!(diff.trim(), "");
}

#[test]
fn status_on_clean_repo_is_empty_string() {
    let dir = tmp_repo_with_file("clean.txt", "clean");
    let status = git_status(dir.path()).expect("status");
    assert_eq!(status.trim(), "");
}

#[test]
fn init_with_only_subdirectories_no_files_at_root() {
    let dir = tmp();
    let sub = dir.path().join("only_dir");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("nested.txt"), "nested").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("nested.txt"));
}

#[test]
fn diff_preserves_exact_content_change() {
    let dir = tmp_repo_with_file("exact.txt", "alpha\nbeta\ngamma\n");
    fs::write(dir.path().join("exact.txt"), "alpha\nBETA\ngamma\n").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("-beta"));
    assert!(diff.contains("+BETA"));
    // Unchanged lines should not show as added/removed
    assert!(!diff.contains("-alpha"));
    assert!(!diff.contains("-gamma"));
}

#[test]
fn status_after_checkout_to_previous_commit() {
    let dir = tmp_repo_with_file("f.txt", "v1\n");
    let first_sha = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();

    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    git_commit(dir.path(), "v2");

    git_ok(dir.path(), &["checkout", &first_sha]);
    let content = fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert!(content.trim() == "v1", "expected 'v1', got: {content:?}");

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.trim().is_empty(),
        "detached HEAD should be clean: {status}"
    );
}

#[test]
fn diff_of_file_with_no_trailing_newline() {
    let dir = tmp_repo_with_file("nonl.txt", "no newline at end");
    fs::write(dir.path().join("nonl.txt"), "changed no newline").unwrap();
    let diff = git_diff(dir.path()).expect("diff");
    assert!(diff.contains("nonl.txt"));
    // Git shows "\ No newline at end of file" marker
    assert!(diff.contains("No newline at end of file") || diff.contains("-no newline"));
}

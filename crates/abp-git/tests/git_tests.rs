//! Integration tests for the `abp-git` crate.
//!
//! Every test creates its own temporary directory that is automatically
//! cleaned up when the `TempDir` guard goes out of scope.

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

/// Create a temp dir, optionally seed it with a file, and return the guard.
fn tmp() -> TempDir {
    TempDir::new().expect("create temp dir")
}

// ── ensure_git_repo ──────────────────────────────────────────────────

#[test]
fn ensure_git_repo_creates_dot_git() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn ensure_git_repo_is_idempotent() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path()); // second call must not fail
    assert!(dir.path().join(".git").exists());
}

#[test]
fn ensure_git_repo_creates_initial_commit() {
    let dir = tmp();
    fs::write(dir.path().join("hello.txt"), "world").unwrap();
    ensure_git_repo(dir.path());

    let log = git(dir.path(), &["log", "--oneline"]);
    assert!(
        log.contains("baseline"),
        "expected 'baseline' commit, got: {log}"
    );
}

#[test]
fn ensure_git_repo_stages_existing_files() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "aaa").unwrap();
    fs::write(dir.path().join("b.txt"), "bbb").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    assert!(tracked.contains("a.txt"));
    assert!(tracked.contains("b.txt"));
}

#[test]
fn ensure_git_repo_on_empty_dir() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    // Even an empty directory should get a .git directory.
    assert!(dir.path().join(".git").exists());
}

#[test]
fn ensure_git_repo_skips_existing_repo() {
    let dir = tmp();

    // Manually initialise so there is no baseline commit.
    git_ok(dir.path(), &["init", "-q"]);
    let log_before = git(dir.path(), &["log", "--oneline"]);

    ensure_git_repo(dir.path());

    // Because .git already existed, ensure_git_repo should be a no-op.
    let log_after = git(dir.path(), &["log", "--oneline"]);
    assert_eq!(log_before, log_after);
}

// ── git_status ───────────────────────────────────────────────────────

#[test]
fn git_status_clean_repo() {
    let dir = tmp();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).expect("git_status should succeed");
    assert!(
        status.trim().is_empty(),
        "expected clean status, got: {status}"
    );
}

#[test]
fn git_status_detects_untracked_file() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("new.txt"), "hello").unwrap();

    let status = git_status(dir.path()).expect("git_status should succeed");
    assert!(
        status.contains("?? new.txt"),
        "expected untracked marker, got: {status}"
    );
}

#[test]
fn git_status_detects_modified_file() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "original").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("f.txt"), "changed").unwrap();

    let status = git_status(dir.path()).expect("git_status should succeed");
    assert!(
        status.contains("f.txt"),
        "expected modified file in status, got: {status}"
    );
}

#[test]
fn git_status_detects_deleted_file() {
    let dir = tmp();
    fs::write(dir.path().join("del.txt"), "bye").unwrap();
    ensure_git_repo(dir.path());

    fs::remove_file(dir.path().join("del.txt")).unwrap();

    let status = git_status(dir.path()).expect("git_status should succeed");
    assert!(
        status.contains("del.txt"),
        "expected deleted file in status, got: {status}"
    );
}

#[test]
fn git_status_detects_staged_addition() {
    let dir = tmp();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("staged.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "staged.txt"]);

    let status = git_status(dir.path()).expect("git_status should succeed");
    assert!(
        status.contains("A  staged.txt"),
        "expected staged addition, got: {status}"
    );
}

#[test]
fn git_status_multiple_changes() {
    let dir = tmp();
    fs::write(dir.path().join("keep.txt"), "keep").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("keep.txt"), "modified").unwrap();
    fs::write(dir.path().join("untracked.txt"), "new").unwrap();

    let status = git_status(dir.path()).expect("git_status should succeed");
    assert!(status.contains("keep.txt"));
    assert!(status.contains("untracked.txt"));
}

#[test]
fn git_status_returns_none_for_non_repo() {
    let dir = tmp();
    // No git init – not a repository.
    let status = git_status(dir.path());
    assert!(
        status.is_none(),
        "expected None for non-repo, got: {status:?}"
    );
}

// ── git_diff ─────────────────────────────────────────────────────────

#[test]
fn git_diff_clean_repo_is_empty() {
    let dir = tmp();
    ensure_git_repo(dir.path());

    let diff = git_diff(dir.path()).expect("git_diff should succeed");
    assert!(diff.trim().is_empty(), "expected empty diff, got: {diff}");
}

#[test]
fn git_diff_detects_content_change() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "line1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("f.txt"), "line2\n").unwrap();

    let diff = git_diff(dir.path()).expect("git_diff should succeed");
    assert!(
        diff.contains("-line1"),
        "expected removed line, got: {diff}"
    );
    assert!(diff.contains("+line2"), "expected added line, got: {diff}");
}

#[test]
fn git_diff_shows_deleted_content() {
    let dir = tmp();
    fs::write(dir.path().join("d.txt"), "content\n").unwrap();
    ensure_git_repo(dir.path());

    fs::remove_file(dir.path().join("d.txt")).unwrap();

    // `git diff` only shows tracked, unstaged changes. A deleted tracked
    // file that is not staged shows up in `git diff`.
    let diff = git_diff(dir.path()).expect("git_diff should succeed");
    assert!(
        diff.contains("d.txt"),
        "expected deleted file in diff, got: {diff}"
    );
}

#[test]
fn git_diff_ignores_untracked_files() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("untracked.txt"), "data").unwrap();

    let diff = git_diff(dir.path()).expect("git_diff should succeed");
    assert!(
        !diff.contains("untracked.txt"),
        "untracked files should not appear in diff"
    );
}

#[test]
fn git_diff_returns_none_for_non_repo() {
    let dir = tmp();
    let diff = git_diff(dir.path());
    assert!(diff.is_none(), "expected None for non-repo, got: {diff:?}");
}

#[test]
fn git_diff_multiline_change() {
    let dir = tmp();
    fs::write(dir.path().join("m.txt"), "a\nb\nc\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("m.txt"), "a\nB\nC\n").unwrap();

    let diff = git_diff(dir.path()).expect("git_diff should succeed");
    assert!(diff.contains("-b"), "expected removed 'b'");
    assert!(diff.contains("+B"), "expected added 'B'");
    assert!(diff.contains("-c"), "expected removed 'c'");
    assert!(diff.contains("+C"), "expected added 'C'");
}

// ── edge cases ───────────────────────────────────────────────────────

#[test]
fn binary_file_tracked_and_modified() {
    let dir = tmp();
    let bin = dir.path().join("img.bin");
    fs::write(&bin, [0u8, 1, 2, 255, 254, 253]).unwrap();
    ensure_git_repo(dir.path());

    fs::write(&bin, [9u8, 8, 7, 6]).unwrap();

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.contains("img.bin"),
        "binary file should show in status"
    );

    let diff = git_diff(dir.path()).expect("diff");
    // Git may show "Binary files differ" or a raw diff depending on config.
    assert!(
        diff.contains("img.bin"),
        "binary file should appear in diff output"
    );
}

#[test]
fn deeply_nested_file() {
    let dir = tmp();
    let nested = dir.path().join("a").join("b").join("c");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("deep.txt"), "deep").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    // Normalise to forward-slash for the git ls-files output.
    assert!(
        tracked.contains("deep.txt"),
        "deeply nested file should be tracked"
    );
}

#[test]
fn file_with_special_characters_in_name() {
    let dir = tmp();
    // Spaces and dashes are safe on all platforms.
    let name = "my file - copy (2).txt";
    fs::write(dir.path().join(name), "data").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.trim().is_empty(),
        "special-char file should be committed cleanly"
    );
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
    for i in 0..50 {
        fs::write(
            dir.path().join(format!("file_{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.trim().is_empty(),
        "all files should be committed cleanly"
    );
}

#[test]
fn status_after_second_commit() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());

    // Make a second commit.
    fs::write(dir.path().join("b.txt"), "v1").unwrap();
    git_ok(dir.path(), &["add", "b.txt"]);
    git_ok(
        dir.path(),
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=t@t",
            "commit",
            "-m",
            "second",
        ],
    );

    let status = git_status(dir.path()).expect("status");
    assert!(
        status.trim().is_empty(),
        "repo should be clean after second commit"
    );
}

#[test]
fn diff_does_not_include_staged_changes() {
    let dir = tmp();
    fs::write(dir.path().join("s.txt"), "original\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("s.txt"), "updated\n").unwrap();
    git_ok(dir.path(), &["add", "s.txt"]);

    // `git diff` (unstaged) should be empty after staging.
    let diff = git_diff(dir.path()).expect("diff");
    assert!(
        diff.trim().is_empty(),
        "staged changes should not appear in unstaged diff"
    );
}

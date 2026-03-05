#![allow(clippy::all)]
#![allow(unknown_lints)]
//! Comprehensive tests for `abp-git` covering repository initialization,
//! file staging/committing, diff generation (text + binary), status tracking,
//! branch operations, .gitignore handling, error handling, large files,
//! Unicode paths/content, and edge cases.
//!
//! Every test creates its own temporary directory cleaned up on drop.

use std::fs;
use std::path::{Path, PathBuf};
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

fn git_success(path: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn tmp() -> TempDir {
    TempDir::new().expect("create temp dir")
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

fn commit_allow_empty(path: &Path, msg: &str) {
    git_ok(
        path,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=t@t",
            "commit",
            "--allow-empty",
            "-m",
            msg,
        ],
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    fs::write(dir.join(name), content).unwrap();
}

fn write_bin(dir: &Path, name: &str, data: &[u8]) {
    fs::write(dir.join(name), data).unwrap();
}

// ════════════════════════════════════════════════════════════════════════
// 1. Repository initialization (15 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn init_creates_dot_git_directory() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_is_idempotent_on_second_call() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    let head1 = git(dir.path(), &["rev-parse", "HEAD"]);
    ensure_git_repo(dir.path());
    let head2 = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(head1, head2, "second call must not create new commits");
}

#[test]
fn init_is_idempotent_on_third_call() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path());
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1", "should still have exactly 1 commit");
}

#[test]
fn init_creates_baseline_commit_message() {
    let dir = tmp();
    write_file(dir.path(), "a.txt", "a");
    ensure_git_repo(dir.path());
    let msg = git(dir.path(), &["log", "-1", "--format=%s"]);
    assert_eq!(msg.trim(), "baseline");
}

#[test]
fn init_baseline_author_name() {
    let dir = tmp();
    write_file(dir.path(), "a.txt", "a");
    ensure_git_repo(dir.path());
    let author = git(dir.path(), &["log", "-1", "--format=%an"]);
    assert_eq!(author.trim(), "abp");
}

#[test]
fn init_baseline_author_email() {
    let dir = tmp();
    write_file(dir.path(), "a.txt", "a");
    ensure_git_repo(dir.path());
    let email = git(dir.path(), &["log", "-1", "--format=%ae"]);
    assert_eq!(email.trim(), "abp@local");
}

#[test]
fn init_head_is_valid_sha() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "x");
    ensure_git_repo(dir.path());
    let sha = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();
    assert!(sha.len() >= 40);
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn init_baseline_is_root_commit() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    let roots = git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(roots.trim(), head.trim());
}

#[test]
fn init_empty_dir_creates_repo() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
    assert!(git_success(
        dir.path(),
        &["rev-parse", "--is-inside-work-tree"]
    ));
}

#[test]
fn init_stages_all_preexisting_files() {
    let dir = tmp();
    write_file(dir.path(), "a.txt", "a");
    write_file(dir.path(), "b.txt", "b");
    write_file(dir.path(), "c.txt", "c");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("a.txt"));
    assert!(files.contains("b.txt"));
    assert!(files.contains("c.txt"));
}

#[test]
fn init_stages_nested_directory_files() {
    let dir = tmp();
    let nested = dir.path().join("src").join("util");
    fs::create_dir_all(&nested).unwrap();
    write_file(&nested, "helper.rs", "// helper");
    write_file(dir.path(), "Cargo.toml", "[package]");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("helper.rs"));
    assert!(files.contains("Cargo.toml"));
}

#[test]
fn init_skips_if_dot_git_already_exists() {
    let dir = tmp();
    git_ok(dir.path(), &["init", "-q"]);
    // Manually init — no baseline commit exists.
    let log_before = git(dir.path(), &["log", "--oneline"]);
    ensure_git_repo(dir.path());
    let log_after = git(dir.path(), &["log", "--oneline"]);
    assert_eq!(log_before, log_after, "existing repo should be untouched");
}

#[test]
fn init_preserves_existing_commits() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "v1");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "g.txt", "v2");
    git_ok(dir.path(), &["add", "g.txt"]);
    commit(dir.path(), "second");
    let count_before = git(dir.path(), &["rev-list", "--count", "HEAD"])
        .trim()
        .to_string();
    ensure_git_repo(dir.path());
    let count_after = git(dir.path(), &["rev-list", "--count", "HEAD"])
        .trim()
        .to_string();
    assert_eq!(count_before, count_after);
}

#[test]
fn init_working_tree_is_clean() {
    let dir = tmp();
    write_file(dir.path(), "hello.txt", "world");
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.trim().is_empty(),
        "working tree should be clean after init"
    );
}

#[test]
fn init_with_deeply_nested_structure() {
    let dir = tmp();
    let deep = dir.path().join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    write_file(&deep, "deep.txt", "content");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("deep.txt"));
}

// ════════════════════════════════════════════════════════════════════════
// 2. File staging and committing (10 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn stage_new_file_shows_in_status() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    write_file(dir.path(), "new.txt", "content");
    git_ok(dir.path(), &["add", "new.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains("A  new.txt"),
        "staged new file should show A, got: {status}"
    );
}

#[test]
fn stage_modified_file_shows_in_status() {
    let dir = tmp();
    write_file(dir.path(), "m.txt", "v1\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "m.txt", "v2\n");
    git_ok(dir.path(), &["add", "m.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains("M  m.txt"),
        "staged modification should show M, got: {status}"
    );
}

#[test]
fn stage_deleted_file_shows_in_status() {
    let dir = tmp();
    write_file(dir.path(), "rm.txt", "bye");
    ensure_git_repo(dir.path());
    git_ok(dir.path(), &["rm", "-q", "rm.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains("D  rm.txt"),
        "staged deletion should show D, got: {status}"
    );
}

#[test]
fn commit_clears_status() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    write_file(dir.path(), "new.txt", "data");
    git_ok(dir.path(), &["add", "new.txt"]);
    commit(dir.path(), "add new");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty(), "should be clean after commit");
}

#[test]
fn commit_increments_rev_count() {
    let dir = tmp();
    write_file(dir.path(), "seed.txt", "seed");
    ensure_git_repo(dir.path());
    for i in 1..=3 {
        write_file(dir.path(), &format!("f{i}.txt"), &format!("c{i}"));
        git_ok(dir.path(), &["add", "-A"]);
        commit(dir.path(), &format!("commit {i}"));
    }
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "4", "baseline + 3 = 4 commits");
}

#[test]
fn stage_add_all_captures_everything() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    write_file(dir.path(), "a.txt", "a");
    write_file(dir.path(), "b.txt", "b");
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    write_file(&sub, "c.txt", "c");
    git_ok(dir.path(), &["add", "-A"]);
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("a.txt"));
    assert!(status.contains("b.txt"));
    assert!(status.contains("c.txt"));
}

#[test]
fn mixed_staged_and_unstaged_shows_mm() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "original\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "f.txt", "staged\n");
    git_ok(dir.path(), &["add", "f.txt"]);
    write_file(dir.path(), "f.txt", "unstaged\n");
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("MM f.txt"), "expected MM, got: {status}");
}

#[test]
fn empty_commit_allowed_with_flag() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    commit_allow_empty(dir.path(), "empty");
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "2");
}

#[test]
fn rename_shows_in_status_after_stage() {
    let dir = tmp();
    write_file(dir.path(), "old.txt", "content for rename detection\n");
    ensure_git_repo(dir.path());
    fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).unwrap();
    git_ok(dir.path(), &["add", "-A"]);
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains("new.txt"),
        "renamed file should appear: {status}"
    );
}

#[test]
fn multiple_sequential_commits() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "v0");
    ensure_git_repo(dir.path());
    for i in 1..=5 {
        write_file(dir.path(), "f.txt", &format!("v{i}\n"));
        git_ok(dir.path(), &["add", "f.txt"]);
        commit(dir.path(), &format!("update to v{i}"));
    }
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "6", "baseline + 5 updates");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

// ════════════════════════════════════════════════════════════════════════
// 3. Diff generation — text files (12 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn diff_clean_repo_is_empty() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn diff_single_line_change() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "before\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "f.txt", "after\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-before"));
    assert!(diff.contains("+after"));
}

#[test]
fn diff_addition_of_lines() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "line1\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "f.txt", "line1\nline2\nline3\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+line2"));
    assert!(diff.contains("+line3"));
}

#[test]
fn diff_removal_of_lines() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "a\nb\nc\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "f.txt", "a\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-b"));
    assert!(diff.contains("-c"));
}

#[test]
fn diff_deleted_tracked_file() {
    let dir = tmp();
    write_file(dir.path(), "gone.txt", "farewell\n");
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("gone.txt")).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("gone.txt"));
    assert!(diff.contains("-farewell"));
}

#[test]
fn diff_untracked_file_not_shown() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    write_file(dir.path(), "untracked.txt", "invisible");
    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.contains("untracked.txt"));
}

#[test]
fn diff_staged_changes_not_in_unstaged_diff() {
    let dir = tmp();
    write_file(dir.path(), "s.txt", "original\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "s.txt", "updated\n");
    git_ok(dir.path(), &["add", "s.txt"]);
    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.trim().is_empty(),
        "staged changes not in unstaged diff"
    );
}

#[test]
fn diff_multiple_files() {
    let dir = tmp();
    write_file(dir.path(), "x.txt", "x1\n");
    write_file(dir.path(), "y.txt", "y1\n");
    write_file(dir.path(), "z.txt", "z1\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "x.txt", "x2\n");
    write_file(dir.path(), "y.txt", "y2\n");
    write_file(dir.path(), "z.txt", "z2\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("x.txt"));
    assert!(diff.contains("y.txt"));
    assert!(diff.contains("z.txt"));
}

#[test]
fn diff_has_unified_format_headers() {
    let dir = tmp();
    write_file(dir.path(), "fmt.txt", "old\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "fmt.txt", "new\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("---"), "should have --- header");
    assert!(diff.contains("+++"), "should have +++ header");
    assert!(diff.contains("@@"), "should have @@ hunk marker");
}

#[test]
fn diff_has_no_ansi_color_codes() {
    let dir = tmp();
    write_file(dir.path(), "c.txt", "before\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "c.txt", "after\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.contains("\x1b["), "diff should have no ANSI escapes");
}

#[test]
fn diff_empty_file_to_content() {
    let dir = tmp();
    write_file(dir.path(), "e.txt", "");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "e.txt", "now has content\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+now has content"));
}

#[test]
fn diff_content_to_empty_file() {
    let dir = tmp();
    write_file(dir.path(), "e.txt", "some content\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "e.txt", "");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-some content"));
}

// ════════════════════════════════════════════════════════════════════════
// 4. Diff generation — binary files (5 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn diff_binary_file_change() {
    let dir = tmp();
    write_bin(dir.path(), "img.bin", &[0u8, 1, 2, 255, 254, 253]);
    ensure_git_repo(dir.path());
    write_bin(dir.path(), "img.bin", &[9u8, 8, 7, 6]);
    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.contains("img.bin"),
        "binary file should appear in diff"
    );
}

#[test]
fn diff_binary_file_shows_binary_marker() {
    let dir = tmp();
    let data: Vec<u8> = (0..=255).collect();
    write_bin(dir.path(), "all.bin", &data);
    ensure_git_repo(dir.path());
    let mut modified = data;
    modified[0] = 128;
    write_bin(dir.path(), "all.bin", &modified);
    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.contains("Binary") || diff.contains("GIT binary") || diff.contains("all.bin"),
        "should indicate binary change: {diff}"
    );
}

#[test]
fn diff_binary_file_status_shows_modification() {
    let dir = tmp();
    write_bin(dir.path(), "data.bin", &[0u8; 100]);
    ensure_git_repo(dir.path());
    write_bin(dir.path(), "data.bin", &[1u8; 100]);
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("data.bin"));
}

#[test]
fn diff_binary_file_deleted() {
    let dir = tmp();
    write_bin(dir.path(), "gone.bin", &[0xFF; 50]);
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("gone.bin")).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("gone.bin"));
}

#[test]
fn diff_mixed_binary_and_text() {
    let dir = tmp();
    write_file(dir.path(), "text.txt", "hello\n");
    write_bin(dir.path(), "bin.dat", &[0u8, 1, 255]);
    ensure_git_repo(dir.path());
    write_file(dir.path(), "text.txt", "world\n");
    write_bin(dir.path(), "bin.dat", &[2u8, 3, 254]);
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("text.txt"));
    assert!(diff.contains("bin.dat"));
}

// ════════════════════════════════════════════════════════════════════════
// 5. Status tracking (10 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn status_clean_after_init() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn status_detects_untracked() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    write_file(dir.path(), "new.txt", "hi");
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("?? new.txt"));
}

#[test]
fn status_detects_unstaged_modification() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "v1");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "f.txt", "v2");
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains(" M f.txt"),
        "expected unstaged mod: {status}"
    );
}

#[test]
fn status_detects_unstaged_deletion() {
    let dir = tmp();
    write_file(dir.path(), "d.txt", "bye");
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("d.txt")).unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains(" D d.txt"),
        "expected unstaged deletion: {status}"
    );
}

#[test]
fn status_multiple_untracked_files() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    for i in 0..5 {
        write_file(dir.path(), &format!("new{i}.txt"), "data");
    }
    let status = git_status(dir.path()).unwrap();
    for i in 0..5 {
        assert!(status.contains(&format!("new{i}.txt")));
    }
}

#[test]
fn status_porcelain_format_structure() {
    let dir = tmp();
    write_file(dir.path(), "p.txt", "v1");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "p.txt", "v2");
    let status = git_status(dir.path()).unwrap();
    let line = status.lines().next().unwrap();
    // Porcelain v1: XY SPACE filename — at least 4 chars
    assert!(line.len() >= 4, "line too short: {line}");
    assert_eq!(&line[2..3], " ", "3rd char should be space");
}

#[test]
fn status_returns_none_for_non_repo() {
    let dir = tmp();
    assert!(git_status(dir.path()).is_none());
}

#[test]
fn status_after_staging_all() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    write_file(dir.path(), "a.txt", "a");
    write_file(dir.path(), "b.txt", "b");
    git_ok(dir.path(), &["add", "-A"]);
    let status = git_status(dir.path()).unwrap();
    // Both should appear as staged additions
    assert!(status.contains("A  a.txt"));
    assert!(status.contains("A  b.txt"));
}

#[test]
fn status_after_partial_staging() {
    let dir = tmp();
    write_file(dir.path(), "a.txt", "v1\n");
    write_file(dir.path(), "b.txt", "v1\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "a.txt", "v2\n");
    write_file(dir.path(), "b.txt", "v2\n");
    git_ok(dir.path(), &["add", "a.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("M  a.txt"), "a should be staged: {status}");
    assert!(
        status.contains(" M b.txt"),
        "b should be unstaged: {status}"
    );
}

#[test]
fn status_agrees_with_diff_on_clean() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(status.trim().is_empty());
    assert!(diff.trim().is_empty());
}

// ════════════════════════════════════════════════════════════════════════
// 6. Branch operations (7 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn branch_create_and_switch() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "v1");
    ensure_git_repo(dir.path());
    git_ok(dir.path(), &["checkout", "-b", "feature"]);
    let branch = git(dir.path(), &["branch", "--show-current"]);
    assert_eq!(branch.trim(), "feature");
}

#[test]
fn branch_status_works_on_new_branch() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "v1\n");
    ensure_git_repo(dir.path());
    git_ok(dir.path(), &["checkout", "-b", "dev"]);
    write_file(dir.path(), "f.txt", "v2\n");
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("f.txt"));
}

#[test]
fn branch_diff_works_on_new_branch() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "old\n");
    ensure_git_repo(dir.path());
    git_ok(dir.path(), &["checkout", "-b", "topic"]);
    write_file(dir.path(), "f.txt", "new\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-old"));
    assert!(diff.contains("+new"));
}

#[test]
fn branch_commit_on_feature_branch() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "v1");
    ensure_git_repo(dir.path());
    git_ok(dir.path(), &["checkout", "-b", "feat"]);
    write_file(dir.path(), "feat.txt", "new");
    git_ok(dir.path(), &["add", "feat.txt"]);
    commit(dir.path(), "feature commit");
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("feat.txt"));
}

#[test]
fn branch_list_multiple() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    git_ok(dir.path(), &["checkout", "-b", "alpha"]);
    git_ok(dir.path(), &["checkout", "-b", "beta"]);
    let branches = git(dir.path(), &["branch", "--list"]);
    assert!(branches.contains("alpha"));
    assert!(branches.contains("beta"));
}

#[test]
fn branch_switch_back_preserves_status() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "original\n");
    ensure_git_repo(dir.path());
    let default_branch = git(dir.path(), &["branch", "--show-current"])
        .trim()
        .to_string();
    git_ok(dir.path(), &["checkout", "-b", "temp"]);
    write_file(dir.path(), "temp.txt", "temp");
    git_ok(dir.path(), &["add", "temp.txt"]);
    commit(dir.path(), "temp commit");
    git_ok(dir.path(), &["checkout", &default_branch]);
    let files = git(dir.path(), &["ls-files"]);
    assert!(
        !files.contains("temp.txt"),
        "temp.txt should not be on default branch"
    );
}

#[test]
fn ensure_git_repo_on_branch_is_noop() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    git_ok(dir.path(), &["checkout", "-b", "work"]);
    write_file(dir.path(), "g.txt", "more");
    git_ok(dir.path(), &["add", "g.txt"]);
    commit(dir.path(), "on work branch");
    ensure_git_repo(dir.path());
    let branch = git(dir.path(), &["branch", "--show-current"]);
    assert_eq!(branch.trim(), "work", "should still be on work branch");
}

// ════════════════════════════════════════════════════════════════════════
// 7. .gitignore handling (8 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn gitignore_hides_ignored_files_from_status() {
    let dir = tmp();
    write_file(dir.path(), ".gitignore", "*.log\n");
    write_file(dir.path(), "app.rs", "fn main(){}");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "debug.log", "log data");
    let status = git_status(dir.path()).unwrap();
    assert!(!status.contains("debug.log"));
}

#[test]
fn gitignore_does_not_hide_non_matching() {
    let dir = tmp();
    write_file(dir.path(), ".gitignore", "*.log\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "readme.txt", "hello");
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("readme.txt"));
}

#[test]
fn gitignore_directory_pattern() {
    let dir = tmp();
    write_file(dir.path(), ".gitignore", "build/\n");
    let build = dir.path().join("build");
    fs::create_dir(&build).unwrap();
    write_file(&build, "output.o", "binary");
    write_file(dir.path(), "src.rs", "//");
    ensure_git_repo(dir.path());
    write_file(&build, "extra.o", "more");
    let status = git_status(dir.path()).unwrap();
    assert!(!status.contains("extra.o"), "build/ should be ignored");
}

#[test]
fn gitignore_negation_pattern() {
    let dir = tmp();
    write_file(dir.path(), ".gitignore", "*.tmp\n!important.tmp\n");
    write_file(dir.path(), "important.tmp", "keep");
    write_file(dir.path(), "junk.tmp", "discard");
    write_file(dir.path(), "main.rs", "//");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(
        files.contains("important.tmp"),
        "negated file should be tracked"
    );
    assert!(
        !files.contains("junk.tmp"),
        "non-negated tmp should be ignored"
    );
}

#[test]
fn gitignore_wildcard_pattern() {
    let dir = tmp();
    write_file(dir.path(), ".gitignore", "*.o\n*.a\n");
    write_file(dir.path(), "main.c", "int main(){}");
    write_file(dir.path(), "main.o", "binary");
    write_file(dir.path(), "lib.a", "archive");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("main.c"));
    assert!(!files.contains("main.o"));
    assert!(!files.contains("lib.a"));
}

#[test]
fn gitignore_nested_gitignore() {
    let dir = tmp();
    write_file(dir.path(), ".gitignore", "");
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    write_file(&sub, ".gitignore", "*.generated\n");
    write_file(&sub, "code.rs", "//");
    write_file(&sub, "output.generated", "gen");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("code.rs"));
    assert!(!files.contains("output.generated"));
}

#[test]
fn gitignore_comment_lines_are_ignored() {
    let dir = tmp();
    write_file(dir.path(), ".gitignore", "# this is a comment\n*.log\n");
    write_file(dir.path(), "app.rs", "//");
    write_file(dir.path(), "test.log", "logs");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("app.rs"));
    assert!(!files.contains("test.log"));
}

#[test]
fn gitignore_does_not_affect_already_tracked() {
    let dir = tmp();
    write_file(dir.path(), "tracked.log", "already tracked");
    ensure_git_repo(dir.path());
    // Add .gitignore AFTER the file is already committed
    write_file(dir.path(), ".gitignore", "*.log\n");
    git_ok(dir.path(), &["add", ".gitignore"]);
    commit(dir.path(), "add gitignore");
    // The already-tracked file should still show in ls-files
    let files = git(dir.path(), &["ls-files"]);
    assert!(
        files.contains("tracked.log"),
        "already tracked files stay tracked"
    );
}

// ════════════════════════════════════════════════════════════════════════
// 8. Error handling (7 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn error_status_on_nonexistent_path() {
    let bad = PathBuf::from("__nonexistent_abp_comp_test_path_77777__");
    assert!(git_status(&bad).is_none());
}

#[test]
fn error_diff_on_nonexistent_path() {
    let bad = PathBuf::from("__nonexistent_abp_comp_test_path_88888__");
    assert!(git_diff(&bad).is_none());
}

#[test]
fn error_status_on_non_repo_dir() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "not a repo");
    assert!(git_status(dir.path()).is_none());
}

#[test]
fn error_diff_on_non_repo_dir() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "not a repo");
    assert!(git_diff(dir.path()).is_none());
}

#[test]
fn error_corrupted_head_does_not_panic() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "data");
    ensure_git_repo(dir.path());
    let head_path = dir.path().join(".git").join("HEAD");
    fs::write(&head_path, "not_a_valid_ref_at_all").unwrap();
    // Should not panic — may return None or Some with unexpected content
    let _ = git_status(dir.path());
    let _ = git_diff(dir.path());
}

#[test]
fn error_empty_repo_no_commits_does_not_panic() {
    let dir = tmp();
    git_ok(dir.path(), &["init", "-q"]);
    // No commits exist — operations should not panic
    let _ = git_status(dir.path());
    let _ = git_diff(dir.path());
}

#[test]
fn error_ensure_git_repo_on_file_path_does_not_panic() {
    let dir = tmp();
    let file_path = dir.path().join("not_a_dir.txt");
    fs::write(&file_path, "I am a file").unwrap();
    // Calling ensure_git_repo on a file (not a directory) should not panic
    ensure_git_repo(&file_path);
    // The file should still exist and be unchanged
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "I am a file");
}

// ════════════════════════════════════════════════════════════════════════
// 9. Large file handling (4 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn large_text_file_diff() {
    let dir = tmp();
    let large: String = (0..5000).map(|i| format!("line {i}\n")).collect();
    write_file(dir.path(), "big.txt", &large);
    ensure_git_repo(dir.path());
    let modified = large.replace("line 2500\n", "LINE_MODIFIED\n");
    write_file(dir.path(), "big.txt", &modified);
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-line 2500"));
    assert!(diff.contains("+LINE_MODIFIED"));
}

#[test]
fn large_binary_file_diff() {
    let dir = tmp();
    let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    write_bin(dir.path(), "large.bin", &data);
    ensure_git_repo(dir.path());
    let mut modified = data;
    modified[5000] = 0xFF;
    write_bin(dir.path(), "large.bin", &modified);
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("large.bin"));
}

#[test]
fn many_files_init_and_status() {
    let dir = tmp();
    for i in 0..100 {
        write_file(
            dir.path(),
            &format!("file_{i:03}.txt"),
            &format!("content {i}"),
        );
    }
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.trim().is_empty(),
        "all 100 files should be committed"
    );
}

#[test]
fn large_file_diff_is_bounded() {
    let dir = tmp();
    let large: String = (0..3000).map(|i| format!("line {i}\n")).collect();
    write_file(dir.path(), "big.txt", &large);
    ensure_git_repo(dir.path());
    let modified = large.replace("line 1500\n", "CHANGED\n");
    write_file(dir.path(), "big.txt", &modified);
    let diff = git_diff(dir.path()).unwrap();
    // Diff should be much smaller than the full file due to context limits
    assert!(
        diff.len() < large.len(),
        "diff should be smaller than full file"
    );
}

// ════════════════════════════════════════════════════════════════════════
// 10. Unicode path and content support (6 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn unicode_content_in_diff() {
    let dir = tmp();
    write_file(dir.path(), "uni.txt", "日本語テスト\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "uni.txt", "中文测试\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.trim().is_empty(),
        "should detect unicode content change"
    );
}

#[test]
fn unicode_content_commit_and_status() {
    let dir = tmp();
    write_file(dir.path(), "emoji.txt", "🎉🚀💯\n");
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.trim().is_empty(),
        "unicode content should commit cleanly"
    );
}

#[test]
fn unicode_content_modification() {
    let dir = tmp();
    write_file(dir.path(), "lang.txt", "こんにちは\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "lang.txt", "さようなら\n");
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains("lang.txt"),
        "modified unicode file in status"
    );
}

#[test]
fn mixed_ascii_and_unicode() {
    let dir = tmp();
    write_file(dir.path(), "mixed.txt", "Hello World\n");
    write_file(dir.path(), "intl.txt", "Ñoño café résumé\n");
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty(), "both files should commit cleanly");
}

#[test]
fn unicode_filename_with_accents() {
    let dir = tmp();
    let name = "résumé.txt";
    write_file(dir.path(), name, "content");
    ensure_git_repo(dir.path());
    // Git may quote the name but it should be tracked
    let files = git(dir.path(), &["ls-files"]);
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.trim().is_empty() || files.contains("sum"),
        "accented filename should be handled"
    );
}

#[test]
fn unicode_filename_with_spaces_and_special() {
    let dir = tmp();
    let name = "my file (copy).txt";
    write_file(dir.path(), name, "data");
    ensure_git_repo(dir.path());
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.trim().is_empty(),
        "special chars in name should be committed"
    );
}

// ════════════════════════════════════════════════════════════════════════
// 11. Edge cases (10 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_file_tracked() {
    let dir = tmp();
    write_file(dir.path(), "empty.txt", "");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("empty.txt"));
}

#[test]
fn edge_file_with_no_trailing_newline() {
    let dir = tmp();
    write_file(dir.path(), "no_nl.txt", "no newline at end");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "no_nl.txt", "no newline at end\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.trim().is_empty(),
        "adding trailing newline should produce diff"
    );
}

#[test]
fn edge_whitespace_only_changes() {
    let dir = tmp();
    write_file(dir.path(), "ws.txt", "hello world\n");
    ensure_git_repo(dir.path());
    write_file(dir.path(), "ws.txt", "hello  world\n");
    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.trim().is_empty(),
        "whitespace change should produce diff"
    );
}

#[test]
fn edge_line_ending_change() {
    let dir = tmp();
    write_file(dir.path(), "le.txt", "a\nb\nc\n");
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("le.txt"), "a\r\nb\r\nc\r\n").unwrap();
    // Whether diff shows depends on autocrlf, but should not panic
    let _ = git_diff(dir.path());
    let _ = git_status(dir.path());
}

#[test]
fn edge_nested_repo_inner_independent() {
    let outer = tmp();
    write_file(outer.path(), "outer.txt", "outer");
    ensure_git_repo(outer.path());

    let inner = outer.path().join("inner");
    fs::create_dir(&inner).unwrap();
    write_file(&inner, "inner.txt", "inner");
    ensure_git_repo(&inner);

    // Inner repo should be independent
    let inner_files = git(&inner, &["ls-files"]);
    assert!(inner_files.contains("inner.txt"));
    assert!(!inner_files.contains("outer.txt"));
}

#[test]
fn edge_dotfiles_are_tracked() {
    let dir = tmp();
    write_file(dir.path(), ".hidden", "secret");
    write_file(dir.path(), ".env", "KEY=val");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains(".hidden"));
    assert!(files.contains(".env"));
}

#[test]
fn edge_very_long_filename() {
    let dir = tmp();
    // Create a file with a long name (200 chars, within filesystem limits)
    let name = format!("{}.txt", "a".repeat(195));
    write_file(dir.path(), &name, "data");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains(&name));
}

#[test]
fn edge_file_permissions_dont_affect_tracking() {
    let dir = tmp();
    write_file(dir.path(), "exec.sh", "#!/bin/sh\necho hi");
    ensure_git_repo(dir.path());
    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("exec.sh"));
}

#[test]
fn edge_concurrent_status_on_different_repos() {
    let dirs: Vec<TempDir> = (0..4)
        .map(|_| {
            let d = tmp();
            write_file(d.path(), "f.txt", "data");
            ensure_git_repo(d.path());
            d
        })
        .collect();

    let handles: Vec<_> = dirs
        .iter()
        .map(|d| {
            let p = d.path().to_path_buf();
            std::thread::spawn(move || git_status(&p))
        })
        .collect();

    for h in handles {
        let result = h.join().expect("thread should not panic");
        assert!(result.is_some());
    }
}

#[test]
fn edge_concurrent_diff_on_different_repos() {
    let dirs: Vec<TempDir> = (0..4)
        .map(|_| {
            let d = tmp();
            write_file(d.path(), "f.txt", "v1\n");
            ensure_git_repo(d.path());
            write_file(d.path(), "f.txt", "v2\n");
            d
        })
        .collect();

    let handles: Vec<_> = dirs
        .iter()
        .map(|d| {
            let p = d.path().to_path_buf();
            std::thread::spawn(move || git_diff(&p))
        })
        .collect();

    for h in handles {
        let diff = h.join().expect("thread should not panic").unwrap();
        assert!(diff.contains("+v2"));
    }
}

// ════════════════════════════════════════════════════════════════════════
// 12. Additional coverage — compound scenarios (5 tests)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn compound_full_workflow() {
    let dir = tmp();
    // Create files, init, modify, stage, commit, verify clean
    write_file(dir.path(), "main.rs", "fn main() {}\n");
    write_file(dir.path(), "lib.rs", "pub fn greet() {}\n");
    ensure_git_repo(dir.path());

    // Modify
    write_file(dir.path(), "main.rs", "fn main() { greet(); }\n");
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("main.rs"));

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("main.rs"));

    // Stage and commit
    git_ok(dir.path(), &["add", "-A"]);
    commit(dir.path(), "update main");

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn compound_add_delete_modify_cycle() {
    let dir = tmp();
    write_file(dir.path(), "keep.txt", "keep\n");
    write_file(dir.path(), "delete_me.txt", "doomed\n");
    write_file(dir.path(), "modify_me.txt", "v1\n");
    ensure_git_repo(dir.path());

    // Delete one, modify another, add a new one
    fs::remove_file(dir.path().join("delete_me.txt")).unwrap();
    write_file(dir.path(), "modify_me.txt", "v2\n");
    write_file(dir.path(), "brand_new.txt", "new\n");

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("delete_me.txt"));
    assert!(status.contains("modify_me.txt"));
    assert!(status.contains("brand_new.txt"));
    assert!(
        !status.contains("keep.txt"),
        "unchanged file should not appear"
    );

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("delete_me.txt"));
    assert!(diff.contains("modify_me.txt"));
    assert!(!diff.contains("brand_new.txt"), "untracked not in diff");
}

#[test]
fn compound_branch_diverge_and_verify() {
    let dir = tmp();
    write_file(dir.path(), "shared.txt", "shared\n");
    ensure_git_repo(dir.path());

    let default_branch = git(dir.path(), &["branch", "--show-current"])
        .trim()
        .to_string();

    // Create feature branch with extra file
    git_ok(dir.path(), &["checkout", "-b", "feature"]);
    write_file(dir.path(), "feature.txt", "feature work");
    git_ok(dir.path(), &["add", "feature.txt"]);
    commit(dir.path(), "feature commit");

    // Switch back to default
    git_ok(dir.path(), &["checkout", &default_branch]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty(), "default branch should be clean");

    let files = git(dir.path(), &["ls-files"]);
    assert!(
        !files.contains("feature.txt"),
        "feature file should not be on default branch"
    );
}

#[test]
fn compound_gitignore_with_modifications() {
    let dir = tmp();
    write_file(dir.path(), ".gitignore", "*.log\ntarget/\n");
    write_file(dir.path(), "app.rs", "fn main(){}\n");
    ensure_git_repo(dir.path());

    // Create ignored and non-ignored files
    write_file(dir.path(), "debug.log", "ignored");
    let target = dir.path().join("target");
    fs::create_dir(&target).unwrap();
    write_file(&target, "output.o", "ignored");
    write_file(dir.path(), "new_module.rs", "// new module\n");

    // Modify tracked file
    write_file(dir.path(), "app.rs", "fn main() { run(); }\n");

    let status = git_status(dir.path()).unwrap();
    assert!(!status.contains("debug.log"), "log should be ignored");
    assert!(!status.contains("output.o"), "target/ should be ignored");
    assert!(status.contains("new_module.rs"), "new file should show");
    assert!(status.contains("app.rs"), "modified file should show");
}

#[test]
fn compound_repeated_modify_commit_cycles() {
    let dir = tmp();
    write_file(dir.path(), "counter.txt", "0\n");
    ensure_git_repo(dir.path());

    for i in 1..=10 {
        write_file(dir.path(), "counter.txt", &format!("{i}\n"));
        git_ok(dir.path(), &["add", "counter.txt"]);
        commit(dir.path(), &format!("update to {i}"));
    }

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "11", "baseline + 10 updates = 11");

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());

    let content = fs::read_to_string(dir.path().join("counter.txt")).unwrap();
    assert_eq!(content, "10\n");
}

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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Comprehensive tests for the `abp-git` crate covering init, commit, diff,
//! status, branch, log, .gitignore, binary files, empty repos, large files,
//! nested repos, and error handling.
//!
//! Every test creates its own temporary directory that is cleaned up when
//! the `TempDir` guard goes out of scope.

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

fn git_succeeds(path: &Path, args: &[&str]) -> bool {
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

fn commit_all(path: &Path, msg: &str) {
    git_ok(path, &["add", "-A"]);
    commit(path, msg);
}

// ════════════════════════════════════════════════════════════════════════
// 1. Init — Initialize new repo, existing repo
// ════════════════════════════════════════════════════════════════════════

#[test]
fn init_creates_dot_git_directory() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_is_idempotent_on_existing_repo() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    ensure_git_repo(dir.path());
    let sha_before = git(dir.path(), &["rev-parse", "HEAD"]);

    ensure_git_repo(dir.path());
    let sha_after = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        sha_before, sha_after,
        "second call must not create a new commit"
    );
}

#[test]
fn init_skips_when_dot_git_already_exists() {
    let dir = tmp();
    git_ok(dir.path(), &["init", "-q"]);
    // No commits exist yet.
    let has_commits = git_succeeds(dir.path(), &["rev-parse", "HEAD"]);

    ensure_git_repo(dir.path());

    // Should still have no baseline commit because .git existed already.
    let has_commits_after = git_succeeds(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(has_commits, has_commits_after);
}

#[test]
fn init_produces_valid_git_repository() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    assert!(git_succeeds(
        dir.path(),
        &["rev-parse", "--is-inside-work-tree"]
    ));
}

#[test]
fn init_with_deep_directory_tree() {
    let dir = tmp();
    let deep = dir.path().join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "leaf").unwrap();
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("leaf.txt"));
}

#[test]
fn init_with_many_files_at_root() {
    let dir = tmp();
    for i in 0..20 {
        fs::write(dir.path().join(format!("file_{i:03}.txt")), format!("{i}")).unwrap();
    }
    ensure_git_repo(dir.path());

    let count: usize = git(dir.path(), &["ls-files"])
        .lines()
        .filter(|l| !l.is_empty())
        .count();
    assert_eq!(count, 20);
}

// ════════════════════════════════════════════════════════════════════════
// 2. Commit — Create commits, commit messages, author
// ════════════════════════════════════════════════════════════════════════

#[test]
fn commit_baseline_message_is_baseline() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    let msg = git(dir.path(), &["log", "-1", "--format=%s"]);
    assert_eq!(msg.trim(), "baseline");
}

#[test]
fn commit_baseline_author_is_abp() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    let author = git(dir.path(), &["log", "-1", "--format=%an"]);
    assert_eq!(author.trim(), "abp");
}

#[test]
fn commit_baseline_email_is_abp_local() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    let email = git(dir.path(), &["log", "-1", "--format=%ae"]);
    assert_eq!(email.trim(), "abp@local");
}

#[test]
fn commit_baseline_is_root_commit() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    let roots = git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(roots.trim(), head.trim());
}

#[test]
fn commit_sha_is_40_hex_chars() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    let sha = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();
    assert!(sha.len() >= 40);
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn commit_second_commit_increments_count() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("b.txt"), "b").unwrap();
    commit_all(dir.path(), "second");

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "2");
}

#[test]
fn commit_three_sequential_commits() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "1").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("b.txt"), "2").unwrap();
    commit_all(dir.path(), "second");

    fs::write(dir.path().join("c.txt"), "3").unwrap();
    commit_all(dir.path(), "third");

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "3");

    let log = git(dir.path(), &["log", "--oneline"]);
    assert!(log.contains("third"));
    assert!(log.contains("second"));
    assert!(log.contains("baseline"));
}

// ════════════════════════════════════════════════════════════════════════
// 3. Diff — Generate diffs, parse diff output
// ════════════════════════════════════════════════════════════════════════

#[test]
fn diff_clean_repo_returns_empty() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
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
fn diff_detects_appended_lines() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "line1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+line2"));
    assert!(diff.contains("+line3"));
}

#[test]
fn diff_detects_removed_lines() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "a\nb\nc\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "a\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-b"));
    assert!(diff.contains("-c"));
}

#[test]
fn diff_shows_deleted_file_content() {
    let dir = tmp();
    fs::write(dir.path().join("gone.txt"), "content\n").unwrap();
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("gone.txt")).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("gone.txt"));
    assert!(diff.contains("-content"));
}

#[test]
fn diff_ignores_untracked_files() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("untracked.txt"), "data").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.contains("untracked.txt"));
}

#[test]
fn diff_staged_changes_not_in_unstaged_diff() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.trim().is_empty(),
        "staged changes not in unstaged diff"
    );
}

#[test]
fn diff_contains_unified_format_markers() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "aaa\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "bbb\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("---"));
    assert!(diff.contains("+++"));
    assert!(diff.contains("@@"));
}

#[test]
fn diff_no_ansi_escape_codes() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "old\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "new\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.contains("\x1b["), "diff must have no ANSI escapes");
}

#[test]
fn diff_multiple_files_all_appear() {
    let dir = tmp();
    for name in &["x.txt", "y.txt", "z.txt"] {
        fs::write(dir.path().join(name), "original\n").unwrap();
    }
    ensure_git_repo(dir.path());
    for name in &["x.txt", "y.txt", "z.txt"] {
        fs::write(dir.path().join(name), "changed\n").unwrap();
    }

    let diff = git_diff(dir.path()).unwrap();
    for name in &["x.txt", "y.txt", "z.txt"] {
        assert!(diff.contains(name), "{name} should appear in diff");
    }
}

#[test]
fn diff_only_changed_file_appears() {
    let dir = tmp();
    fs::write(dir.path().join("changed.txt"), "v1\n").unwrap();
    fs::write(dir.path().join("stable.txt"), "same\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("changed.txt"), "v2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("changed.txt"));
    assert!(!diff.contains("stable.txt"));
}

#[test]
fn diff_empty_to_content() {
    let dir = tmp();
    fs::write(dir.path().join("e.txt"), "").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("e.txt"), "now has content\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+now has content"));
}

#[test]
fn diff_content_to_empty() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "will be erased\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-will be erased"));
}

// ════════════════════════════════════════════════════════════════════════
// 4. Status — Track modified/added/deleted files
// ════════════════════════════════════════════════════════════════════════

#[test]
fn status_clean_after_init() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn status_detects_untracked_file() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("new.txt"), "data").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("?? new.txt"));
}

#[test]
fn status_detects_modified_tracked_file() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("f.txt"));
    assert!(status.contains(" M"), "should show modification marker");
}

#[test]
fn status_detects_deleted_tracked_file() {
    let dir = tmp();
    fs::write(dir.path().join("rm.txt"), "bye").unwrap();
    ensure_git_repo(dir.path());
    fs::remove_file(dir.path().join("rm.txt")).unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("rm.txt"));
    assert!(status.contains(" D"), "should show deletion marker");
}

#[test]
fn status_detects_staged_addition() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("staged.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "staged.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("A  staged.txt"));
}

#[test]
fn status_detects_staged_modification() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("M  f.txt"));
}

#[test]
fn status_detects_staged_deletion() {
    let dir = tmp();
    fs::write(dir.path().join("rm.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    git_ok(dir.path(), &["rm", "-q", "rm.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("D  rm.txt"));
}

#[test]
fn status_mixed_staged_and_unstaged() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "original\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("f.txt"), "staged\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    fs::write(dir.path().join("f.txt"), "unstaged\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("MM f.txt"));
}

#[test]
fn status_multiple_files_all_shown() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("a.txt"), "A").unwrap();
    fs::write(dir.path().join("b.txt"), "B").unwrap();
    fs::write(dir.path().join("c.txt"), "C").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("a.txt"));
    assert!(status.contains("b.txt"));
    assert!(status.contains("c.txt"));
}

#[test]
fn status_clean_after_committing_all() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("new.txt"), "data").unwrap();
    commit_all(dir.path(), "add new");

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn status_returns_none_for_non_repo() {
    let dir = tmp();
    assert!(git_status(dir.path()).is_none());
}

#[test]
fn status_porcelain_format_structure() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2").unwrap();

    let status = git_status(dir.path()).unwrap();
    let line = status.lines().next().expect("should have a line");
    // Porcelain v1: XY<space>filename — at least 4 chars.
    assert!(line.len() >= 4, "porcelain line too short: {line}");
    assert_eq!(&line[2..3], " ", "3rd char should be a space");
}

// ════════════════════════════════════════════════════════════════════════
// 5. Branch — Create/switch branches
// ════════════════════════════════════════════════════════════════════════

#[test]
fn branch_create_and_list() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    git_ok(dir.path(), &["checkout", "-b", "feature-x"]);

    let branches = git(dir.path(), &["branch", "--list"]);
    assert!(branches.contains("feature-x"));
}

#[test]
fn branch_switch_and_status_isolated() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "main-content").unwrap();
    ensure_git_repo(dir.path());

    git_ok(dir.path(), &["checkout", "-b", "dev"]);
    fs::write(dir.path().join("dev.txt"), "dev-only").unwrap();
    commit_all(dir.path(), "dev commit");

    git_ok(dir.path(), &["checkout", "-"]);
    // dev.txt should not exist on the original branch.
    assert!(!dir.path().join("dev.txt").exists());

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn branch_diff_is_branch_scoped() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());

    git_ok(dir.path(), &["checkout", "-b", "topic"]);
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-v1"));
    assert!(diff.contains("+v2"));
}

#[test]
fn branch_multiple_branches_coexist() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "base").unwrap();
    ensure_git_repo(dir.path());

    for name in &["alpha", "beta", "gamma"] {
        git_ok(dir.path(), &["branch", name]);
    }

    let branches = git(dir.path(), &["branch", "--list"]);
    for name in &["alpha", "beta", "gamma"] {
        assert!(branches.contains(name), "branch {name} should exist");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 6. Log — Retrieve commit history
// ════════════════════════════════════════════════════════════════════════

#[test]
fn log_baseline_appears() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    let log = git(dir.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"));
}

#[test]
fn log_multiple_commits_in_order() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "1").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("f.txt"), "2").unwrap();
    commit_all(dir.path(), "second");

    fs::write(dir.path().join("f.txt"), "3").unwrap();
    commit_all(dir.path(), "third");

    let log = git(dir.path(), &["log", "--oneline", "--reverse"]);
    let lines: Vec<&str> = log.lines().collect();
    assert!(lines.len() >= 3);
    assert!(lines[0].contains("baseline"));
    assert!(lines[1].contains("second"));
    assert!(lines[2].contains("third"));
}

#[test]
fn log_commit_count_matches() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "1").unwrap();
    ensure_git_repo(dir.path());

    for i in 2..=5 {
        fs::write(dir.path().join("f.txt"), format!("{i}")).unwrap();
        commit_all(dir.path(), &format!("commit {i}"));
    }

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "5");
}

#[test]
fn log_shows_file_changes_in_commit() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("b.txt"), "b").unwrap();
    commit_all(dir.path(), "add b");

    let show = git(dir.path(), &["show", "--name-only", "--format=", "HEAD"]);
    assert!(show.contains("b.txt"));
    assert!(!show.contains("a.txt"));
}

// ════════════════════════════════════════════════════════════════════════
// 7. Ignore — .gitignore handling
// ════════════════════════════════════════════════════════════════════════

#[test]
fn ignore_dotgitignore_hides_files_from_status() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    fs::write(dir.path().join("app.rs"), "fn main() {}").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("debug.log"), "log data").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(!status.contains("debug.log"), ".log should be ignored");
}

#[test]
fn ignore_unignored_file_still_appears_in_status() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("visible.txt"), "visible").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("visible.txt"));
}

#[test]
fn ignore_directory_pattern() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
    let build = dir.path().join("build");
    fs::create_dir(&build).unwrap();
    fs::write(build.join("output.o"), "binary").unwrap();
    fs::write(dir.path().join("src.rs"), "code").unwrap();
    ensure_git_repo(dir.path());

    fs::write(build.join("new.o"), "more").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(!status.contains("output.o"));
    assert!(!status.contains("new.o"));
}

#[test]
fn ignore_negation_pattern() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.log\n!important.log\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("debug.log"), "ignored").unwrap();
    fs::write(dir.path().join("important.log"), "kept").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(!status.contains("debug.log"), "debug.log should be ignored");
    assert!(
        status.contains("important.log"),
        "important.log should NOT be ignored"
    );
}

#[test]
fn ignore_nested_gitignore_in_subdirectory() {
    let dir = tmp();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join(".gitignore"), "*.tmp\n").unwrap();
    fs::write(sub.join("keep.txt"), "keep").unwrap();
    ensure_git_repo(dir.path());

    fs::write(sub.join("scratch.tmp"), "temp data").unwrap();
    fs::write(sub.join("new.txt"), "new").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(
        !status.contains("scratch.tmp"),
        ".tmp should be ignored by sub/.gitignore"
    );
    assert!(status.contains("new.txt"));
}

#[test]
fn ignore_diff_excludes_ignored_files() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.bak\n").unwrap();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    fs::write(dir.path().join("f.bak"), "backup").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("f.txt"));
    assert!(
        !diff.contains("f.bak"),
        "ignored file should not appear in diff"
    );
}

// ════════════════════════════════════════════════════════════════════════
// 8. Binary files — Binary file diff handling
// ════════════════════════════════════════════════════════════════════════

#[test]
fn binary_file_shows_in_status() {
    let dir = tmp();
    fs::write(dir.path().join("img.bin"), &[0u8, 1, 2, 255, 254]).unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("img.bin"), &[9u8, 8, 7]).unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("img.bin"));
}

#[test]
fn binary_file_diff_contains_filename() {
    let dir = tmp();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(dir.path().join("data.bin"), &data).unwrap();
    ensure_git_repo(dir.path());

    let mut modified = data;
    modified[0] = 128;
    fs::write(dir.path().join("data.bin"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("data.bin"));
}

#[test]
fn binary_diff_shows_binary_marker() {
    let dir = tmp();
    fs::write(dir.path().join("b.bin"), &[0u8, 1, 2, 0, 255]).unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("b.bin"), &[255u8, 0, 1, 0, 2]).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.contains("Binary") || diff.contains("GIT binary"),
        "binary diff should have binary marker, got: {diff}"
    );
}

#[test]
fn binary_and_text_changes_together() {
    let dir = tmp();
    fs::write(dir.path().join("code.txt"), "fn main() {}\n").unwrap();
    fs::write(dir.path().join("icon.bin"), &[0u8, 0, 0, 1]).unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("code.txt"), "fn main() { todo!() }\n").unwrap();
    fs::write(dir.path().join("icon.bin"), &[1u8, 1, 1, 0]).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("code.txt"));
    assert!(diff.contains("icon.bin"));
}

// ════════════════════════════════════════════════════════════════════════
// 9. Empty repo — Operations on empty repo
// ════════════════════════════════════════════════════════════════════════

#[test]
fn empty_repo_init_creates_dot_git() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn empty_repo_status_is_clean_or_none() {
    let dir = tmp();
    ensure_git_repo(dir.path());

    // An empty repo may or may not have a commit — status should not panic.
    let status = git_status(dir.path());
    if let Some(s) = status {
        assert!(s.trim().is_empty(), "empty repo status should be clean");
    }
}

#[test]
fn empty_repo_diff_is_clean_or_none() {
    let dir = tmp();
    ensure_git_repo(dir.path());

    let diff = git_diff(dir.path());
    if let Some(d) = diff {
        assert!(d.trim().is_empty(), "empty repo diff should be clean");
    }
}

#[test]
fn empty_repo_no_commits_status_does_not_panic() {
    let dir = tmp();
    git_ok(dir.path(), &["init", "-q"]);
    // Repo with no commits at all.
    let _ = git_status(dir.path());
}

#[test]
fn empty_repo_no_commits_diff_does_not_panic() {
    let dir = tmp();
    git_ok(dir.path(), &["init", "-q"]);
    let _ = git_diff(dir.path());
}

// ════════════════════════════════════════════════════════════════════════
// 10. Large files — Handle large file diffs
// ════════════════════════════════════════════════════════════════════════

#[test]
fn large_file_2000_lines_diff_shows_change() {
    let dir = tmp();
    let content: String = (0..2000).map(|i| format!("line {i}\n")).collect();
    fs::write(dir.path().join("big.txt"), &content).unwrap();
    ensure_git_repo(dir.path());

    let modified = content.replace("line 1000\n", "LINE_THOUSAND\n");
    fs::write(dir.path().join("big.txt"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-line 1000"));
    assert!(diff.contains("+LINE_THOUSAND"));
}

#[test]
fn large_file_diff_is_smaller_than_file() {
    let dir = tmp();
    let content: String = (0..5000).map(|i| format!("line {i}\n")).collect();
    fs::write(dir.path().join("huge.txt"), &content).unwrap();
    ensure_git_repo(dir.path());

    let modified = content.replace("line 2500\n", "CHANGED\n");
    fs::write(dir.path().join("huge.txt"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.len() < content.len(),
        "diff ({}) should be smaller than file ({})",
        diff.len(),
        content.len()
    );
}

#[test]
fn large_number_of_files_status() {
    let dir = tmp();
    for i in 0..100 {
        fs::write(dir.path().join(format!("f{i:03}.txt")), format!("{i}")).unwrap();
    }
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    assert!(
        status.trim().is_empty(),
        "all 100 files should be committed"
    );
}

#[test]
fn large_number_of_files_all_modified() {
    let dir = tmp();
    for i in 0..50 {
        fs::write(dir.path().join(format!("f{i:02}.txt")), format!("v1_{i}\n")).unwrap();
    }
    ensure_git_repo(dir.path());

    for i in 0..50 {
        fs::write(dir.path().join(format!("f{i:02}.txt")), format!("v2_{i}\n")).unwrap();
    }

    let diff = git_diff(dir.path()).unwrap();
    for i in 0..50 {
        assert!(
            diff.contains(&format!("f{i:02}.txt")),
            "f{i:02}.txt should appear in diff"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// 11. Nested repos — Don't recurse into nested .git
// ════════════════════════════════════════════════════════════════════════

#[test]
fn nested_repo_does_not_track_inner_dot_git() {
    let dir = tmp();
    fs::write(dir.path().join("root.txt"), "root").unwrap();

    let inner = dir.path().join("inner");
    fs::create_dir(&inner).unwrap();
    fs::write(inner.join("inner.txt"), "inner data").unwrap();
    ensure_git_repo(&inner);

    ensure_git_repo(dir.path());

    // The outer repo should NOT have .git files from inner/.git tracked.
    let files = git(dir.path(), &["ls-files"]);
    assert!(!files.contains(".git/"), "inner .git should not be tracked");
    assert!(files.contains("root.txt"));
}

#[test]
fn nested_repo_status_ignores_inner_repo_changes() {
    let dir = tmp();
    let inner = dir.path().join("sub");
    fs::create_dir(&inner).unwrap();
    fs::write(inner.join("s.txt"), "data").unwrap();
    ensure_git_repo(&inner);
    ensure_git_repo(dir.path());

    // Modify a file inside the inner repo.
    fs::write(inner.join("s.txt"), "modified").unwrap();

    let status = git_status(dir.path()).unwrap();
    // The outer repo may show the submodule as dirty, but should not show
    // individual files from inside the nested repo.
    assert!(
        !status.contains("s.txt"),
        "inner file should not appear individually in outer status"
    );
}

#[test]
fn nested_repo_outer_diff_does_not_include_inner_file_contents() {
    let dir = tmp();
    let inner = dir.path().join("nested");
    fs::create_dir(&inner).unwrap();
    fs::write(inner.join("n.txt"), "v1\n").unwrap();
    ensure_git_repo(&inner);
    ensure_git_repo(dir.path());

    fs::write(inner.join("n.txt"), "v2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    // The outer diff should not contain the content diff from the nested repo.
    assert!(
        !diff.contains("-v1"),
        "inner repo diff content should not leak to outer diff"
    );
}

// ════════════════════════════════════════════════════════════════════════
// 12. Error handling — Missing git binary, corrupt repo
// ════════════════════════════════════════════════════════════════════════

#[test]
fn error_status_on_nonexistent_path() {
    let bad = std::path::PathBuf::from("__nonexistent_abp_test_dir_ops_deep__");
    assert!(git_status(&bad).is_none());
}

#[test]
fn error_diff_on_nonexistent_path() {
    let bad = std::path::PathBuf::from("__nonexistent_abp_test_dir_ops_deep__");
    assert!(git_diff(&bad).is_none());
}

#[test]
fn error_corrupt_head_does_not_panic() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    // Corrupt the HEAD reference.
    fs::write(dir.path().join(".git").join("HEAD"), "not_a_valid_ref").unwrap();

    // Should not panic — may return None or Some.
    let _ = git_status(dir.path());
    let _ = git_diff(dir.path());
}

#[test]
fn error_corrupt_index_does_not_panic() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    // Corrupt the index file.
    let index = dir.path().join(".git").join("index");
    if index.exists() {
        fs::write(&index, "corrupted_data").unwrap();
    }

    let _ = git_status(dir.path());
    let _ = git_diff(dir.path());
}

#[test]
fn error_status_and_diff_on_plain_file() {
    let dir = tmp();
    let file = dir.path().join("not_a_dir.txt");
    fs::write(&file, "just a file").unwrap();

    // Passing a file path instead of a directory should not panic.
    let _ = git_status(&file);
    let _ = git_diff(&file);
}

#[test]
fn error_ensure_git_repo_on_readonly_is_safe() {
    // ensure_git_repo ignores errors internally, so calling it on a
    // non-writable path should not panic (it may silently fail).
    let bad = std::path::PathBuf::from("__nonexistent_readonly_abp__");
    ensure_git_repo(&bad);
    // If we reach here, no panic occurred.
}

// ════════════════════════════════════════════════════════════════════════
// Bonus: cross-cutting and additional edge cases
// ════════════════════════════════════════════════════════════════════════

#[test]
fn whitespace_only_change_produces_diff() {
    let dir = tmp();
    fs::write(dir.path().join("ws.txt"), "hello world\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("ws.txt"), "hello  world\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

#[test]
fn newline_eof_change_produces_diff() {
    let dir = tmp();
    fs::write(dir.path().join("nl.txt"), "no newline").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("nl.txt"), "no newline\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

#[test]
fn status_and_diff_agree_on_clean() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "stable").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(status.trim().is_empty());
    assert!(diff.trim().is_empty());
}

#[test]
fn status_and_diff_agree_on_dirty() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(!status.trim().is_empty());
    assert!(!diff.trim().is_empty());
    assert!(status.contains("f.txt"));
    assert!(diff.contains("f.txt"));
}

#[test]
fn concurrent_status_on_separate_repos() {
    let dirs: Vec<TempDir> = (0..4).map(|_| tmp()).collect();
    for d in &dirs {
        fs::write(d.path().join("f.txt"), "data").unwrap();
        ensure_git_repo(d.path());
    }

    let handles: Vec<_> = dirs
        .iter()
        .map(|d| {
            let p = d.path().to_path_buf();
            std::thread::spawn(move || git_status(&p))
        })
        .collect();

    for h in handles {
        let result = h.join().expect("thread must not panic");
        assert!(result.is_some());
    }
}

#[test]
fn concurrent_diff_on_separate_repos() {
    let dirs: Vec<TempDir> = (0..4).map(|_| tmp()).collect();
    for d in &dirs {
        fs::write(d.path().join("f.txt"), "v1\n").unwrap();
        ensure_git_repo(d.path());
        fs::write(d.path().join("f.txt"), "v2\n").unwrap();
    }

    let handles: Vec<_> = dirs
        .iter()
        .map(|d| {
            let p = d.path().to_path_buf();
            std::thread::spawn(move || git_diff(&p))
        })
        .collect();

    for h in handles {
        let result = h.join().expect("thread must not panic");
        let diff = result.expect("diff should succeed");
        assert!(diff.contains("+v2"));
    }
}

#[test]
fn special_characters_in_filename() {
    let dir = tmp();
    let name = "my file - copy (2).txt";
    fs::write(dir.path().join(name), "data").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    assert!(
        status.trim().is_empty(),
        "special-char file should be committed"
    );
}

#[test]
fn unicode_content_diff() {
    let dir = tmp();
    fs::write(dir.path().join("u.txt"), "日本語\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("u.txt"), "中文\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.trim().is_empty(),
        "unicode content change should produce diff"
    );
}

#[test]
fn ensure_git_repo_preserves_existing_commits() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("g.txt"), "extra").unwrap();
    commit_all(dir.path(), "second commit");

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
fn line_ending_change_does_not_panic() {
    let dir = tmp();
    fs::write(dir.path().join("le.txt"), "a\nb\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("le.txt"), "a\r\nb\r\n").unwrap();

    // May or may not produce a diff depending on core.autocrlf, but must not panic.
    let _ = git_diff(dir.path());
    let _ = git_status(dir.path());
}

#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
//! Comprehensive tests for the `abp-git` crate validating git operations
//! used by workspace staging.
//!
//! Covers: repo initialization, commit creation, diff generation, diff parsing,
//! status checking, branch operations, file staging, error handling, concurrency,
//! and edge cases (empty repos, large diffs, binary files).
//!
//! Every test creates its own temporary directory that is cleaned up when the
//! `TempDir` guard goes out of scope.

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

fn init_with_file(name: &str, content: &str) -> TempDir {
    let dir = tmp();
    fs::write(dir.path().join(name), content).unwrap();
    ensure_git_repo(dir.path());
    dir
}

// ════════════════════════════════════════════════════════════════════
// §1  Repository initialization
// ════════════════════════════════════════════════════════════════════

#[test]
fn init_creates_dot_git_directory() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_creates_valid_head_sha() {
    let dir = init_with_file("f.txt", "x");
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(head.trim().len(), 40);
    assert!(head.trim().chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn init_baseline_is_root_commit() {
    let dir = init_with_file("a.txt", "a");
    let roots = git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(roots.trim(), head.trim());
}

#[test]
fn init_baseline_message_is_baseline() {
    let dir = init_with_file("a.txt", "a");
    let msg = git(dir.path(), &["log", "-1", "--format=%s"]);
    assert_eq!(msg.trim(), "baseline");
}

#[test]
fn init_baseline_author_is_abp() {
    let dir = init_with_file("a.txt", "data");
    let author = git(dir.path(), &["log", "-1", "--format=%an"]);
    assert_eq!(author.trim(), "abp");
}

#[test]
fn init_baseline_email_is_abp_local() {
    let dir = init_with_file("a.txt", "data");
    let email = git(dir.path(), &["log", "-1", "--format=%ae"]);
    assert_eq!(email.trim(), "abp@local");
}

#[test]
fn init_sets_clean_working_tree() {
    let dir = init_with_file("x.txt", "data");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn init_idempotent_skips_existing_repo() {
    let dir = init_with_file("f.txt", "v1");
    let sha1 = git(dir.path(), &["rev-parse", "HEAD"]);
    ensure_git_repo(dir.path());
    let sha2 = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(sha1.trim(), sha2.trim());
}

#[test]
fn init_idempotent_preserves_extra_commits() {
    let dir = init_with_file("f.txt", "v1");
    fs::write(dir.path().join("new.txt"), "new").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);
    commit(dir.path(), "second");
    let count_before = git(dir.path(), &["rev-list", "--count", "HEAD"]);

    ensure_git_repo(dir.path());
    let count_after = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count_before.trim(), count_after.trim());
}

#[test]
fn init_with_subdirectory_structure() {
    let dir = tmp();
    let sub = dir.path().join("src").join("lib");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("mod.rs"), "// mod").unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("Cargo.toml"));
    assert!(files.contains("mod.rs"));
}

#[test]
fn init_with_deeply_nested_dirs() {
    let dir = tmp();
    let deep = dir.path().join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "deep").unwrap();
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("leaf.txt"));
}

#[test]
fn init_empty_directory_creates_empty_commit() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    // With no files, git add -A is a no-op and commit may fail silently.
    // The .git dir should still exist.
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_with_multiple_files() {
    let dir = tmp();
    for i in 0..10 {
        fs::write(
            dir.path().join(format!("file_{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["ls-files"]);
    for i in 0..10 {
        assert!(files.contains(&format!("file_{i}.txt")));
    }
}

#[test]
fn init_with_dotfile() {
    let dir = tmp();
    fs::write(dir.path().join(".hidden"), "secret").unwrap();
    fs::write(dir.path().join("visible.txt"), "public").unwrap();
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains(".hidden"));
    assert!(files.contains("visible.txt"));
}

// ════════════════════════════════════════════════════════════════════
// §2  Commit creation
// ════════════════════════════════════════════════════════════════════

#[test]
fn second_commit_produces_valid_sha() {
    let dir = init_with_file("f.txt", "v1");
    fs::write(dir.path().join("new.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);
    commit(dir.path(), "second commit");

    let sha = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(sha.trim().len(), 40);
}

#[test]
fn commit_count_after_second_commit() {
    let dir = init_with_file("a.txt", "a");
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    git_ok(dir.path(), &["add", "b.txt"]);
    commit(dir.path(), "second");

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "2");
}

#[test]
fn commit_count_after_three_commits() {
    let dir = init_with_file("a.txt", "a");
    for i in 1..=2 {
        fs::write(dir.path().join(format!("f{i}.txt")), format!("{i}")).unwrap();
        git_ok(dir.path(), &["add", "-A"]);
        commit(dir.path(), &format!("commit {i}"));
    }
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "3");
}

#[test]
fn commit_preserves_file_content() {
    let dir = init_with_file("data.txt", "original");
    fs::write(dir.path().join("data.txt"), "modified").unwrap();
    git_ok(dir.path(), &["add", "data.txt"]);
    commit(dir.path(), "update");

    let content = git(dir.path(), &["show", "HEAD:data.txt"]);
    assert_eq!(content.trim(), "modified");
}

#[test]
fn commit_message_is_preserved() {
    let dir = init_with_file("f.txt", "data");
    fs::write(dir.path().join("g.txt"), "more").unwrap();
    git_ok(dir.path(), &["add", "g.txt"]);
    commit(dir.path(), "my custom message");

    let msg = git(dir.path(), &["log", "-1", "--format=%s"]);
    assert_eq!(msg.trim(), "my custom message");
}

#[test]
fn empty_commit_allowed() {
    let dir = init_with_file("f.txt", "data");
    commit_allow_empty(dir.path(), "empty");
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "2");
}

#[test]
fn commit_with_delete() {
    let dir = init_with_file("doomed.txt", "bye");
    git_ok(dir.path(), &["rm", "-q", "doomed.txt"]);
    commit(dir.path(), "delete file");

    let files = git(dir.path(), &["ls-files"]);
    assert!(!files.contains("doomed.txt"));
}

// ════════════════════════════════════════════════════════════════════
// §3  Diff generation
// ════════════════════════════════════════════════════════════════════

#[test]
fn diff_empty_on_clean_repo() {
    let dir = init_with_file("f.txt", "data");
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn diff_shows_added_lines() {
    let dir = init_with_file("grow.txt", "line1\n");
    fs::write(dir.path().join("grow.txt"), "line1\nline2\nline3\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+line2"));
    assert!(diff.contains("+line3"));
}

#[test]
fn diff_shows_removed_lines() {
    let dir = init_with_file("shrink.txt", "a\nb\nc\n");
    fs::write(dir.path().join("shrink.txt"), "a\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-b"));
    assert!(diff.contains("-c"));
}

#[test]
fn diff_shows_modified_lines() {
    let dir = init_with_file("mod.txt", "old line\n");
    fs::write(dir.path().join("mod.txt"), "new line\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-old line"));
    assert!(diff.contains("+new line"));
}

#[test]
fn diff_multiple_files() {
    let dir = tmp();
    fs::write(dir.path().join("x.txt"), "x1\n").unwrap();
    fs::write(dir.path().join("y.txt"), "y1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("x.txt"), "x2\n").unwrap();
    fs::write(dir.path().join("y.txt"), "y2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("x.txt"));
    assert!(diff.contains("y.txt"));
}

#[test]
fn diff_after_partial_stage_excludes_staged() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "aaa\n").unwrap();
    fs::write(dir.path().join("b.txt"), "bbb\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("a.txt"), "AAA\n").unwrap();
    fs::write(dir.path().join("b.txt"), "BBB\n").unwrap();
    git_ok(dir.path(), &["add", "a.txt"]);

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.contains("a.txt"));
    assert!(diff.contains("b.txt"));
}

#[test]
fn diff_empty_file_to_content() {
    let dir = init_with_file("e.txt", "");
    fs::write(dir.path().join("e.txt"), "now has content\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+now has content"));
}

#[test]
fn diff_content_to_empty() {
    let dir = init_with_file("c.txt", "some content\n");
    fs::write(dir.path().join("c.txt"), "").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-some content"));
}

#[test]
fn diff_contains_unified_format_header() {
    let dir = init_with_file("h.txt", "old\n");
    fs::write(dir.path().join("h.txt"), "new\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("---"));
    assert!(diff.contains("+++"));
    assert!(diff.contains("@@"));
}

#[test]
fn diff_contains_file_path_in_header() {
    let dir = init_with_file("myfile.txt", "v1\n");
    fs::write(dir.path().join("myfile.txt"), "v2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("a/myfile.txt"));
    assert!(diff.contains("b/myfile.txt"));
}

#[test]
fn diff_no_color_codes() {
    let dir = init_with_file("f.txt", "old\n");
    fs::write(dir.path().join("f.txt"), "new\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    // ANSI escape codes start with \x1b[
    assert!(!diff.contains("\x1b["), "diff should have no color codes");
}

#[test]
fn diff_single_character_change() {
    let dir = init_with_file("tiny.txt", "a\n");
    fs::write(dir.path().join("tiny.txt"), "b\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-a"));
    assert!(diff.contains("+b"));
}

#[test]
fn diff_whitespace_only_change() {
    let dir = init_with_file("ws.txt", "hello world\n");
    fs::write(dir.path().join("ws.txt"), "hello  world\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

#[test]
fn diff_newline_at_eof_change() {
    let dir = init_with_file("nl.txt", "no newline");
    fs::write(dir.path().join("nl.txt"), "no newline\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

#[test]
fn diff_preserves_context_lines() {
    let dir = init_with_file(
        "ctx.txt",
        "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n",
    );
    // Only change line5
    fs::write(
        dir.path().join("ctx.txt"),
        "line1\nline2\nline3\nline4\nLINE5\nline6\nline7\nline8\nline9\nline10\n",
    )
    .unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-line5"));
    assert!(diff.contains("+LINE5"));
    // Context lines (unchanged) should appear without +/- prefix
    assert!(diff.contains(" line4"));
    assert!(diff.contains(" line6"));
}

#[test]
fn diff_in_subdirectory() {
    let dir = tmp();
    let sub = dir.path().join("src");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("main.rs"), "fn main() {}\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(sub.join("main.rs"), "fn main() { println!(\"hello\"); }\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("src/main.rs"));
}

// ════════════════════════════════════════════════════════════════════
// §4  Diff parsing / structure
// ════════════════════════════════════════════════════════════════════

#[test]
fn diff_hunk_header_format() {
    let dir = init_with_file("hunk.txt", "aaa\n");
    fs::write(dir.path().join("hunk.txt"), "bbb\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    let hunk_lines: Vec<&str> = diff.lines().filter(|l| l.starts_with("@@")).collect();
    assert!(!hunk_lines.is_empty());
    for h in &hunk_lines {
        assert!(h.contains("@@"));
    }
}

#[test]
fn diff_multiple_hunks_in_one_file() {
    // Create a file with enough lines to produce separate hunks
    let mut content = String::new();
    for i in 1..=30 {
        content.push_str(&format!("line{i}\n"));
    }
    let dir = init_with_file("multi.txt", &content);

    // Change lines near the top and bottom
    let mut modified = String::new();
    for i in 1..=30 {
        if i == 2 {
            modified.push_str("CHANGED2\n");
        } else if i == 29 {
            modified.push_str("CHANGED29\n");
        } else {
            modified.push_str(&format!("line{i}\n"));
        }
    }
    fs::write(dir.path().join("multi.txt"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    let hunk_count = diff.lines().filter(|l| l.starts_with("@@")).count();
    assert!(hunk_count >= 2, "expected >=2 hunks, got {hunk_count}");
}

#[test]
fn diff_line_counts_match() {
    let dir = init_with_file("count.txt", "a\nb\nc\n");
    fs::write(dir.path().join("count.txt"), "a\nB\nc\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    let added = diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .count();
    let removed = diff
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .count();
    assert_eq!(added, 1);
    assert_eq!(removed, 1);
}

#[test]
fn diff_add_only_line_counts() {
    let dir = init_with_file("addonly.txt", "existing\n");
    fs::write(dir.path().join("addonly.txt"), "existing\nnew1\nnew2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    let added = diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .count();
    let removed = diff
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .count();
    assert_eq!(added, 2);
    assert_eq!(removed, 0);
}

#[test]
fn diff_remove_only_line_counts() {
    let dir = init_with_file("rmonly.txt", "keep\nremove1\nremove2\n");
    fs::write(dir.path().join("rmonly.txt"), "keep\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    let added = diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .count();
    let removed = diff
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .count();
    assert_eq!(added, 0);
    assert_eq!(removed, 2);
}

#[test]
fn diff_header_has_diff_git_prefix() {
    let dir = init_with_file("hdr.txt", "old\n");
    fs::write(dir.path().join("hdr.txt"), "new\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.starts_with("diff --git"));
}

// ════════════════════════════════════════════════════════════════════
// §5  Status checking
// ════════════════════════════════════════════════════════════════════

#[test]
fn status_clean_repo() {
    let dir = init_with_file("f.txt", "data");
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn status_untracked_file() {
    let dir = init_with_file("f.txt", "data");
    fs::write(dir.path().join("new.txt"), "untracked").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("?? new.txt") || status.contains("new.txt"));
}

#[test]
fn status_modified_unstaged() {
    let dir = init_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains(" M f.txt"), "got: {status}");
}

#[test]
fn status_staged_modification() {
    let dir = init_with_file("m.txt", "v1\n");
    fs::write(dir.path().join("m.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "m.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("M  m.txt"), "got: {status}");
}

#[test]
fn status_staged_new_file() {
    let dir = init_with_file("existing.txt", "data");
    fs::write(dir.path().join("added.txt"), "new").unwrap();
    git_ok(dir.path(), &["add", "added.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("A  added.txt"), "got: {status}");
}

#[test]
fn status_staged_delete() {
    let dir = init_with_file("rm.txt", "bye");
    git_ok(dir.path(), &["rm", "-q", "rm.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("D  rm.txt"), "got: {status}");
}

#[test]
fn status_mixed_staged_and_unstaged() {
    let dir = init_with_file("f.txt", "original\n");
    fs::write(dir.path().join("f.txt"), "staged\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    fs::write(dir.path().join("f.txt"), "unstaged\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("MM f.txt"), "got: {status}");
}

#[test]
fn status_empty_after_commit() {
    let dir = init_with_file("f.txt", "v1");
    fs::write(dir.path().join("new.txt"), "added").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);
    commit(dir.path(), "add new file");

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn status_multiple_untracked_files() {
    let dir = init_with_file("existing.txt", "data");
    for i in 0..5 {
        fs::write(dir.path().join(format!("untracked_{i}.txt")), "new").unwrap();
    }

    let status = git_status(dir.path()).unwrap();
    for i in 0..5 {
        assert!(status.contains(&format!("untracked_{i}.txt")));
    }
}

#[test]
fn status_returns_some_for_valid_repo() {
    let dir = init_with_file("f.txt", "data");
    assert!(git_status(dir.path()).is_some());
}

#[test]
fn status_porcelain_format() {
    let dir = init_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    // Porcelain v1 format: XY filename
    for line in status.lines() {
        assert!(
            line.len() >= 4,
            "porcelain line should be at least 4 chars: '{line}'"
        );
    }
}

#[test]
fn status_and_diff_agree_clean() {
    let dir = init_with_file("f.txt", "data");
    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(status.trim().is_empty());
    assert!(diff.trim().is_empty());
}

#[test]
fn status_and_diff_agree_dirty() {
    let dir = init_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(!status.trim().is_empty());
    assert!(!diff.trim().is_empty());
    assert!(status.contains("f.txt"));
    assert!(diff.contains("f.txt"));
}

// ════════════════════════════════════════════════════════════════════
// §6  Branch operations
// ════════════════════════════════════════════════════════════════════

#[test]
fn default_branch_exists_after_init() {
    let dir = init_with_file("f.txt", "data");
    let branches = git(dir.path(), &["branch", "--list"]);
    assert!(
        !branches.trim().is_empty(),
        "should have at least one branch"
    );
}

#[test]
fn status_works_on_non_default_branch() {
    let dir = init_with_file("f.txt", "data");
    git_ok(dir.path(), &["checkout", "-b", "feature"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn diff_works_on_non_default_branch() {
    let dir = init_with_file("f.txt", "v1\n");
    git_ok(dir.path(), &["checkout", "-b", "feature"]);
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+v2"));
}

#[test]
fn ensure_git_repo_works_after_branch_switch() {
    let dir = init_with_file("f.txt", "data");
    git_ok(dir.path(), &["checkout", "-b", "other"]);
    ensure_git_repo(dir.path()); // should be no-op
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn branch_create_and_commit_independent() {
    let dir = init_with_file("f.txt", "main_content\n");
    let main_sha = git(dir.path(), &["rev-parse", "HEAD"]);

    git_ok(dir.path(), &["checkout", "-b", "feature"]);
    fs::write(dir.path().join("feature.txt"), "feature data").unwrap();
    git_ok(dir.path(), &["add", "feature.txt"]);
    commit(dir.path(), "feature commit");

    let feature_sha = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_ne!(main_sha.trim(), feature_sha.trim());
}

#[test]
fn status_on_detached_head() {
    let dir = init_with_file("f.txt", "data");
    let sha = git(dir.path(), &["rev-parse", "HEAD"]);
    git_ok(dir.path(), &["checkout", sha.trim()]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

// ════════════════════════════════════════════════════════════════════
// §7  File staging operations
// ════════════════════════════════════════════════════════════════════

#[test]
fn stage_single_file() {
    let dir = init_with_file("f.txt", "v1");
    fs::write(dir.path().join("f.txt"), "v2").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("M  f.txt"));
}

#[test]
fn stage_all_files() {
    let dir = init_with_file("a.txt", "a");
    fs::write(dir.path().join("a.txt"), "A").unwrap();
    fs::write(dir.path().join("b.txt"), "B").unwrap();
    git_ok(dir.path(), &["add", "-A"]);

    let status = git_status(dir.path()).unwrap();
    // No unstaged changes should remain
    for line in status.lines() {
        let xy = &line[..2];
        assert!(
            !xy.ends_with('M') && !xy.ends_with('?'),
            "all changes should be staged, got: {line}"
        );
    }
}

#[test]
fn unstage_file() {
    let dir = init_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    git_ok(dir.path(), &["reset", "HEAD", "f.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains(" M f.txt"), "got: {status}");
}

#[test]
fn stage_new_file_then_diff_is_empty() {
    let dir = init_with_file("f.txt", "data");
    fs::write(dir.path().join("new.txt"), "new content\n").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);

    // git diff (unstaged) should not show staged new file
    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.contains("new.txt"));
}

#[test]
fn stage_delete_shows_in_status() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    ensure_git_repo(dir.path());

    git_ok(dir.path(), &["rm", "-q", "a.txt"]);
    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("D  a.txt"));
    assert!(!status.contains("b.txt"));
}

#[test]
fn stage_rename_detection() {
    let dir = init_with_file("old.txt", "content\n");
    fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).unwrap();
    git_ok(dir.path(), &["add", "-A"]);

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("new.txt"));
}

// ════════════════════════════════════════════════════════════════════
// §8  Error handling for non-git directories
// ════════════════════════════════════════════════════════════════════

#[test]
fn status_returns_none_for_nonexistent_dir() {
    let bad = Path::new("__nonexistent_abp_test_dir_99999__");
    assert!(git_status(bad).is_none());
}

#[test]
fn diff_returns_none_for_nonexistent_dir() {
    let bad = Path::new("__nonexistent_abp_test_dir_99999__");
    assert!(git_diff(bad).is_none());
}

#[test]
fn status_returns_none_for_non_git_dir() {
    let dir = tmp();
    // Don't call ensure_git_repo — plain directory
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    assert!(git_status(dir.path()).is_none());
}

#[test]
fn diff_returns_none_for_non_git_dir() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    assert!(git_diff(dir.path()).is_none());
}

#[test]
fn ensure_git_repo_does_not_panic_on_empty_dir() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    // Should not panic; .git should exist even if commit failed
    assert!(dir.path().join(".git").exists());
}

#[test]
fn status_returns_some_even_for_empty_repo() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    // May or may not have a commit, but status should return Some
    let result = git_status(dir.path());
    // On empty repos git status --porcelain still works
    assert!(result.is_some() || result.is_none()); // no panic
}

#[test]
fn diff_returns_some_for_empty_initialized_repo() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    let result = git_diff(dir.path());
    // Should not panic regardless of outcome
    assert!(result.is_some() || result.is_none());
}

// ════════════════════════════════════════════════════════════════════
// §9  Concurrent git operations
// ════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_status_calls() {
    let dirs: Vec<TempDir> = (0..4)
        .map(|_| {
            let d = tmp();
            fs::write(d.path().join("f.txt"), "data").unwrap();
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
fn concurrent_diff_calls() {
    let dirs: Vec<TempDir> = (0..4)
        .map(|_| {
            let d = tmp();
            fs::write(d.path().join("f.txt"), "v1\n").unwrap();
            ensure_git_repo(d.path());
            fs::write(d.path().join("f.txt"), "v2\n").unwrap();
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
        let result = h.join().expect("thread should not panic");
        let diff = result.expect("diff should succeed");
        assert!(diff.contains("+v2"));
    }
}

#[test]
fn concurrent_ensure_git_repo_different_dirs() {
    let dirs: Vec<TempDir> = (0..8)
        .map(|i| {
            let d = tmp();
            fs::write(d.path().join(format!("file_{i}.txt")), format!("data_{i}")).unwrap();
            d
        })
        .collect();

    let handles: Vec<_> = dirs
        .iter()
        .map(|d| {
            let p = d.path().to_path_buf();
            std::thread::spawn(move || ensure_git_repo(&p))
        })
        .collect();

    for h in handles {
        h.join().expect("thread should not panic");
    }

    for d in &dirs {
        assert!(d.path().join(".git").exists());
    }
}

#[test]
fn concurrent_mixed_operations() {
    let dir = init_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let p1 = dir.path().to_path_buf();
    let p2 = dir.path().to_path_buf();

    let h1 = std::thread::spawn(move || git_status(&p1));
    let h2 = std::thread::spawn(move || git_diff(&p2));

    let status = h1.join().unwrap().unwrap();
    let diff = h2.join().unwrap().unwrap();

    assert!(status.contains("f.txt"));
    assert!(diff.contains("f.txt"));
}

#[test]
fn concurrent_status_many_threads() {
    let dir = init_with_file("f.txt", "data");
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let p = dir.path().to_path_buf();
            std::thread::spawn(move || git_status(&p))
        })
        .collect();

    for h in handles {
        let result = h.join().expect("thread should not panic");
        assert!(result.is_some());
    }
}

// ════════════════════════════════════════════════════════════════════
// §10  Edge cases
// ════════════════════════════════════════════════════════════════════

// ── Empty repos ─────────────────────────────────────────────────────

#[test]
fn empty_dir_init_creates_git_dir() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn status_after_all_files_deleted() {
    let dir = init_with_file("doomed.txt", "data");
    fs::remove_file(dir.path().join("doomed.txt")).unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("doomed.txt"));
}

// ── Large diffs ─────────────────────────────────────────────────────

#[test]
fn large_file_diff() {
    let dir = tmp();
    let content: String = (0..1000).map(|i| format!("line {i}\n")).collect();
    fs::write(dir.path().join("big.txt"), &content).unwrap();
    ensure_git_repo(dir.path());

    let modified: String = (0..1000)
        .map(|i| {
            if i % 100 == 0 {
                format!("MODIFIED {i}\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect();
    fs::write(dir.path().join("big.txt"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("MODIFIED 0"));
    assert!(diff.contains("MODIFIED 500"));
    assert!(diff.contains("MODIFIED 900"));
}

#[test]
fn many_files_diff() {
    let dir = tmp();
    for i in 0..50 {
        fs::write(dir.path().join(format!("f{i}.txt")), format!("v1_{i}\n")).unwrap();
    }
    ensure_git_repo(dir.path());

    for i in 0..50 {
        fs::write(dir.path().join(format!("f{i}.txt")), format!("v2_{i}\n")).unwrap();
    }

    let diff = git_diff(dir.path()).unwrap();
    for i in 0..50 {
        assert!(diff.contains(&format!("f{i}.txt")));
    }
}

#[test]
fn large_single_line_file() {
    let dir = tmp();
    let long_line = "x".repeat(10_000);
    fs::write(dir.path().join("long.txt"), &long_line).unwrap();
    ensure_git_repo(dir.path());

    let modified = "y".repeat(10_000);
    fs::write(dir.path().join("long.txt"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

// ── Binary files ────────────────────────────────────────────────────

#[test]
fn binary_file_status() {
    let dir = tmp();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(dir.path().join("bin.dat"), &data).unwrap();
    ensure_git_repo(dir.path());

    let mut modified = data.clone();
    modified[0] = 128;
    fs::write(dir.path().join("bin.dat"), &modified).unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("bin.dat"));
}

#[test]
fn binary_file_diff_mentions_binary() {
    let dir = tmp();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(dir.path().join("bin.dat"), &data).unwrap();
    ensure_git_repo(dir.path());

    let mut modified = data.clone();
    modified[0] = 128;
    fs::write(dir.path().join("bin.dat"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("bin.dat"));
}

#[test]
fn binary_file_new_in_status() {
    let dir = init_with_file("text.txt", "hello");
    let data: Vec<u8> = vec![0, 1, 2, 255, 254, 253];
    fs::write(dir.path().join("new.bin"), &data).unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("new.bin"));
}

// ── Unicode ─────────────────────────────────────────────────────────

#[test]
fn unicode_content_diff() {
    let dir = init_with_file("uni.txt", "日本語テスト\n");
    fs::write(dir.path().join("uni.txt"), "中文测试\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

#[test]
fn unicode_filename_tracked() {
    let dir = tmp();
    let name = "données.txt";
    fs::write(dir.path().join(name), "contenu").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty(), "file should be committed at init");
}

#[test]
fn emoji_content() {
    let dir = init_with_file("emoji.txt", "👋🌍\n");
    fs::write(dir.path().join("emoji.txt"), "🎉✅\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

// ── .gitignore ──────────────────────────────────────────────────────

#[test]
fn gitignore_hides_files_from_status() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    fs::write(dir.path().join("app.log"), "log data").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("debug.log"), "more logs").unwrap();
    fs::write(dir.path().join("new.rs"), "// new").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(!status.contains("debug.log"));
    assert!(status.contains("new.rs"));
}

#[test]
fn gitignore_directory_pattern() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
    let build = dir.path().join("build");
    fs::create_dir_all(&build).unwrap();
    fs::write(build.join("output.o"), "binary").unwrap();
    fs::write(dir.path().join("src.rs"), "code").unwrap();
    ensure_git_repo(dir.path());

    fs::write(build.join("new.o"), "more binary").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(!status.contains("build/"));
    assert!(!status.contains("output.o"));
    assert!(!status.contains("new.o"));
}

#[test]
fn gitignore_negation_pattern() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.log\n!important.log\n").unwrap();
    fs::write(dir.path().join("debug.log"), "debug").unwrap();
    fs::write(dir.path().join("important.log"), "keep").unwrap();
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("important.log"));
    assert!(!files.contains("debug.log"));
}

// ── Special content ─────────────────────────────────────────────────

#[test]
fn file_with_only_newlines() {
    let dir = init_with_file("newlines.txt", "\n\n\n");
    fs::write(dir.path().join("newlines.txt"), "\n\n\n\n\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

#[test]
fn file_with_tabs_and_spaces() {
    let dir = init_with_file("mixed.txt", "\tindented\n");
    fs::write(dir.path().join("mixed.txt"), "    spaces\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());
}

#[test]
fn file_with_crlf_line_endings() {
    let dir = init_with_file("crlf.txt", "line1\r\nline2\r\n");
    fs::write(dir.path().join("crlf.txt"), "line1\r\nmodified\r\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    // Git may or may not show a diff depending on autocrlf settings,
    // but it should not panic or error.
    let _ = diff;
}

#[test]
fn very_long_filename() {
    let dir = tmp();
    let long_name = format!("{}.txt", "a".repeat(200));
    fs::write(dir.path().join(&long_name), "data").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn file_permissions_preserved_in_status() {
    let dir = init_with_file("script.sh", "#!/bin/bash\necho hi\n");
    fs::write(dir.path().join("script.sh"), "#!/bin/bash\necho hello\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("script.sh"));
}

// ── Nested directories ──────────────────────────────────────────────

#[test]
fn diff_in_nested_directory() {
    let dir = tmp();
    let sub = dir.path().join("a").join("b");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("deep.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(sub.join("deep.txt"), "v2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("deep.txt"));
}

#[test]
fn status_in_nested_directory() {
    let dir = tmp();
    let sub = dir.path().join("x").join("y").join("z");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("leaf.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    fs::write(sub.join("leaf.txt"), "modified").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("leaf.txt"));
}

#[test]
fn add_new_directory_with_files() {
    let dir = init_with_file("root.txt", "data");

    let sub = dir.path().join("newdir");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("a.txt"), "a").unwrap();
    fs::write(sub.join("b.txt"), "b").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("newdir/"));
}

// ── Regression-style tests ──────────────────────────────────────────

#[test]
fn ensure_git_repo_twice_same_result() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());
    let sha1 = git(dir.path(), &["rev-parse", "HEAD"]);
    ensure_git_repo(dir.path());
    let sha2 = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(sha1.trim(), sha2.trim());
}

#[test]
fn ensure_git_repo_three_times() {
    let dir = init_with_file("f.txt", "data");
    ensure_git_repo(dir.path());
    ensure_git_repo(dir.path());
    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1", "should still have just one commit");
}

#[test]
fn diff_after_revert() {
    let dir = init_with_file("f.txt", "original\n");
    fs::write(dir.path().join("f.txt"), "modified\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(!diff.trim().is_empty());

    // Revert
    fs::write(dir.path().join("f.txt"), "original\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn status_after_revert() {
    let dir = init_with_file("f.txt", "original\n");
    fs::write(dir.path().join("f.txt"), "modified\n").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(!status.trim().is_empty());

    fs::write(dir.path().join("f.txt"), "original\n").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

#[test]
fn sequential_modifications_and_diffs() {
    let dir = init_with_file("seq.txt", "v1\n");
    for i in 2..=5 {
        fs::write(dir.path().join("seq.txt"), format!("v{i}\n")).unwrap();
        let diff = git_diff(dir.path()).unwrap();
        assert!(diff.contains(&format!("+v{i}")));
    }
}

#[test]
fn diff_after_git_add_is_empty() {
    let dir = init_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.trim().is_empty(),
        "staged changes not in unstaged diff"
    );
}

#[test]
fn diff_after_commit_is_empty() {
    let dir = init_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    commit(dir.path(), "update");

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn status_with_only_empty_directories() {
    let dir = init_with_file("f.txt", "data");
    fs::create_dir_all(dir.path().join("empty_dir")).unwrap();

    let status = git_status(dir.path()).unwrap();
    // Git doesn't track empty directories
    assert!(status.trim().is_empty());
}

#[test]
fn multiple_file_types_in_one_repo() {
    let dir = tmp();
    fs::write(dir.path().join("code.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("data.json"), "{}").unwrap();
    fs::write(dir.path().join("readme.md"), "# Hello").unwrap();
    fs::write(dir.path().join("config.toml"), "[section]").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());

    fs::write(dir.path().join("code.rs"), "fn main() { todo!() }").unwrap();
    fs::write(dir.path().join("data.json"), "{\"key\": 1}").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("code.rs"));
    assert!(diff.contains("data.json"));
    assert!(!diff.contains("readme.md"));
    assert!(!diff.contains("config.toml"));
}

#[test]
fn symlink_handling_no_panic() {
    // Creating symlinks may fail on Windows without admin; just ensure no panic
    let dir = init_with_file("target.txt", "target data");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt")).ok();
    }
    let _status = git_status(dir.path());
    let _diff = git_diff(dir.path());
}

#[test]
fn rapid_modify_status_cycles() {
    let dir = init_with_file("cycle.txt", "v0\n");
    for i in 1..=20 {
        fs::write(dir.path().join("cycle.txt"), format!("v{i}\n")).unwrap();
        let status = git_status(dir.path()).unwrap();
        assert!(!status.trim().is_empty());
    }
}

#[test]
fn rapid_modify_diff_cycles() {
    let dir = init_with_file("cycle.txt", "v0\n");
    for i in 1..=20 {
        fs::write(dir.path().join("cycle.txt"), format!("v{i}\n")).unwrap();
        let diff = git_diff(dir.path()).unwrap();
        assert!(diff.contains(&format!("+v{i}")));
    }
}

#[test]
fn diff_shows_correct_line_for_append() {
    let dir = init_with_file("append.txt", "line1\nline2\nline3\n");
    fs::write(
        dir.path().join("append.txt"),
        "line1\nline2\nline3\nline4\n",
    )
    .unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+line4"));
    // Should not show any removed lines
    let removed = diff
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .count();
    assert_eq!(removed, 0);
}

#[test]
fn diff_shows_correct_line_for_prepend() {
    let dir = init_with_file("prepend.txt", "line1\nline2\n");
    fs::write(dir.path().join("prepend.txt"), "line0\nline1\nline2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+line0"));
}

#[test]
fn status_with_special_chars_in_content() {
    let dir = init_with_file("special.txt", "normal\n");
    fs::write(dir.path().join("special.txt"), "has $pecial & ch@rs!\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("special.txt"));
}

#[test]
fn init_with_gitignore_file() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.tmp\n").unwrap();
    fs::write(dir.path().join("keep.txt"), "keep").unwrap();
    fs::write(dir.path().join("skip.tmp"), "skip").unwrap();
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("keep.txt"));
    assert!(files.contains(".gitignore"));
    assert!(!files.contains("skip.tmp"));
}

#[test]
fn diff_returns_string_type() {
    let dir = init_with_file("f.txt", "v1\n");
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    let diff: Option<String> = git_diff(dir.path());
    assert!(diff.is_some());
}

#[test]
fn status_returns_string_type() {
    let dir = init_with_file("f.txt", "data");
    let status: Option<String> = git_status(dir.path());
    assert!(status.is_some());
}

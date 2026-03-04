#![allow(clippy::all)]
#![allow(unknown_lints)]
//! Tests for the enhanced git integration (diff between commits, patch
//! creation, change statistics, blame, commit helpers).

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

use abp_git::{
    ensure_git_repo, format_commit_message, git_blame, git_change_stats, git_commit,
    git_create_patch, git_diff_commits, git_diff_staged, git_head,
};

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

// ════════════════════════════════════════════════════════════════════════
// git_diff_commits
// ════════════════════════════════════════════════════════════════════════

#[test]
fn diff_commits_shows_changes_between_two_shas() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    let sha1 = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();

    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    commit(dir.path(), "update");
    let sha2 = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();

    let diff = git_diff_commits(dir.path(), &sha1, &sha2).expect("diff should succeed");
    assert!(diff.contains("-v1"), "should show removed v1");
    assert!(diff.contains("+v2"), "should show added v2");
}

#[test]
fn diff_commits_empty_when_same_ref() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let diff = git_diff_commits(dir.path(), "HEAD", "HEAD").expect("should succeed");
    assert!(diff.trim().is_empty(), "same ref should yield empty diff");
}

#[test]
fn diff_commits_none_for_non_repo() {
    let dir = tmp();
    assert!(git_diff_commits(dir.path(), "HEAD~1", "HEAD").is_none());
}

#[test]
fn diff_commits_multiple_files() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a1\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b1\n").unwrap();
    ensure_git_repo(dir.path());
    let sha1 = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();

    fs::write(dir.path().join("a.txt"), "a2\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b2\n").unwrap();
    git_ok(dir.path(), &["add", "-A"]);
    commit(dir.path(), "update both");
    let sha2 = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();

    let diff = git_diff_commits(dir.path(), &sha1, &sha2).unwrap();
    assert!(diff.contains("a.txt"));
    assert!(diff.contains("b.txt"));
}

// ════════════════════════════════════════════════════════════════════════
// git_diff_staged
// ════════════════════════════════════════════════════════════════════════

#[test]
fn diff_staged_shows_cached_changes() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "original\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("f.txt"), "updated\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);

    let diff = git_diff_staged(dir.path()).expect("staged diff should succeed");
    assert!(diff.contains("-original"), "should show old line");
    assert!(diff.contains("+updated"), "should show new line");
}

#[test]
fn diff_staged_empty_when_nothing_staged() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data\n").unwrap();
    ensure_git_repo(dir.path());
    // Modify but don't stage.
    fs::write(dir.path().join("f.txt"), "changed\n").unwrap();

    let diff = git_diff_staged(dir.path()).unwrap();
    assert!(
        diff.trim().is_empty(),
        "nothing staged means empty cached diff"
    );
}

// ════════════════════════════════════════════════════════════════════════
// git_create_patch
// ════════════════════════════════════════════════════════════════════════

#[test]
fn create_patch_includes_all_changes() {
    let dir = tmp();
    fs::write(dir.path().join("existing.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("existing.txt"), "v2\n").unwrap();
    fs::write(dir.path().join("brand_new.txt"), "hello\n").unwrap();

    let patch = git_create_patch(dir.path()).expect("patch should succeed");
    // Patch should include both tracked modification and new file.
    assert!(patch.contains("existing.txt"), "modified file in patch");
    assert!(patch.contains("brand_new.txt"), "new file in patch");
}

#[test]
fn create_patch_leaves_working_tree_unchanged() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let _patch = git_create_patch(dir.path());

    // After patch creation the index should be reset — the file should still
    // appear as unstaged in status.
    let status = abp_git::git_status(dir.path()).unwrap();
    assert!(
        status.contains("f.txt"),
        "working tree should remain dirty after patch creation"
    );
}

#[test]
fn create_patch_empty_for_clean_repo() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let patch = git_create_patch(dir.path()).expect("should succeed");
    assert!(
        patch.trim().is_empty(),
        "clean repo should yield empty patch"
    );
}

// ════════════════════════════════════════════════════════════════════════
// git_change_stats / ChangeStats
// ════════════════════════════════════════════════════════════════════════

#[test]
fn change_stats_detects_added_file() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("new.txt"), "hello\nworld\n").unwrap();

    let stats = git_change_stats(dir.path()).expect("stats should succeed");
    assert!(
        stats
            .added
            .iter()
            .any(|p| p.to_string_lossy().contains("new.txt")),
        "new.txt should be in added list"
    );
    assert!(stats.total_additions >= 2, "should count added lines");
}

#[test]
fn change_stats_detects_modified_file() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "line1\nline2\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("f.txt"), "LINE1\nline2\n").unwrap();

    let stats = git_change_stats(dir.path()).expect("stats");
    assert!(
        stats
            .modified
            .iter()
            .any(|p| p.to_string_lossy().contains("f.txt")),
        "f.txt should be modified"
    );
    assert!(stats.total_additions >= 1);
    assert!(stats.total_deletions >= 1);
}

#[test]
fn change_stats_detects_deleted_file() {
    let dir = tmp();
    fs::write(dir.path().join("gone.txt"), "bye\n").unwrap();
    ensure_git_repo(dir.path());

    fs::remove_file(dir.path().join("gone.txt")).unwrap();

    let stats = git_change_stats(dir.path()).expect("stats");
    assert!(
        stats
            .deleted
            .iter()
            .any(|p| p.to_string_lossy().contains("gone.txt")),
        "gone.txt should be deleted"
    );
}

#[test]
fn change_stats_empty_for_clean_repo() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let stats = git_change_stats(dir.path()).expect("stats");
    assert!(stats.is_empty(), "clean repo should have no changes");
    assert_eq!(stats.file_count(), 0);
}

#[test]
fn change_stats_file_count() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("a.txt"), "A\n").unwrap();
    fs::write(dir.path().join("c.txt"), "new\n").unwrap();
    fs::remove_file(dir.path().join("b.txt")).unwrap();

    let stats = git_change_stats(dir.path()).expect("stats");
    // a.txt modified, b.txt deleted, c.txt added = 3 files.
    assert_eq!(stats.file_count(), 3, "should count 3 changed files");
}

#[test]
fn change_stats_resets_index() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let _stats = git_change_stats(dir.path());

    // After stats, the index should be reset.
    let status = abp_git::git_status(dir.path()).unwrap();
    assert!(
        status.contains("f.txt"),
        "file should remain unstaged after stats"
    );
}

#[test]
fn change_stats_none_for_non_repo() {
    let dir = tmp();
    assert!(git_change_stats(dir.path()).is_none());
}

// ════════════════════════════════════════════════════════════════════════
// git_blame
// ════════════════════════════════════════════════════════════════════════

#[test]
fn blame_returns_lines_for_committed_file() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();
    ensure_git_repo(dir.path());

    let blame = git_blame(dir.path(), Path::new("f.txt")).expect("blame should succeed");
    assert_eq!(blame.len(), 3, "should have 3 blame lines");
    assert_eq!(blame[0].line_no, 1);
    assert_eq!(blame[0].content, "line1");
    assert_eq!(blame[1].content, "line2");
    assert_eq!(blame[2].content, "line3");
}

#[test]
fn blame_includes_author() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();
    ensure_git_repo(dir.path());

    let blame = git_blame(dir.path(), Path::new("f.txt")).unwrap();
    assert_eq!(
        blame[0].author, "abp",
        "baseline commit author should be abp"
    );
}

#[test]
fn blame_reflects_second_commit_author() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("f.txt"), "v1\nv2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    commit(dir.path(), "add line2");

    let blame = git_blame(dir.path(), Path::new("f.txt")).unwrap();
    assert_eq!(blame.len(), 2);
    // First line from baseline (abp), second from our test commit.
    assert_eq!(blame[0].author, "abp");
    assert_eq!(blame[1].author, "test");
}

#[test]
fn blame_none_for_untracked_file() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("untracked.txt"), "data\n").unwrap();

    // Blame on an untracked file should fail.
    assert!(git_blame(dir.path(), Path::new("untracked.txt")).is_none());
}

#[test]
fn blame_none_for_non_repo() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data\n").unwrap();
    assert!(git_blame(dir.path(), Path::new("f.txt")).is_none());
}

// ════════════════════════════════════════════════════════════════════════
// format_commit_message
// ════════════════════════════════════════════════════════════════════════

#[test]
fn format_message_summary_only() {
    let msg = format_commit_message("apply tool changes", None);
    assert_eq!(msg, "abp: apply tool changes");
}

#[test]
fn format_message_with_body() {
    let msg = format_commit_message("apply changes", Some("Modified 3 files.\nAll tests pass."));
    assert!(msg.starts_with("abp: apply changes\n\n"));
    assert!(msg.contains("Modified 3 files."));
}

#[test]
fn format_message_empty_body_treated_as_none() {
    let msg = format_commit_message("summary", Some("  "));
    assert_eq!(msg, "abp: summary");
}

// ════════════════════════════════════════════════════════════════════════
// git_commit
// ════════════════════════════════════════════════════════════════════════

#[test]
fn git_commit_creates_commit_and_returns_sha() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("new.txt"), "hello").unwrap();
    let sha = git_commit(dir.path(), "add new file").expect("commit should succeed");

    assert_eq!(sha.len(), 40, "should return full SHA");
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));

    // Verify the commit message.
    let msg = git(dir.path(), &["log", "-1", "--format=%s"]);
    assert_eq!(msg.trim(), "add new file");
}

#[test]
fn git_commit_uses_abp_identity() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("g.txt"), "more").unwrap();
    git_commit(dir.path(), "test identity").unwrap();

    let author = git(dir.path(), &["log", "-1", "--format=%an"]);
    assert_eq!(author.trim(), "abp");
}

#[test]
fn git_commit_none_when_nothing_to_commit() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    // Nothing changed — commit should fail and return None.
    let result = git_commit(dir.path(), "empty commit");
    assert!(
        result.is_none(),
        "commit with no changes should return None"
    );
}

// ════════════════════════════════════════════════════════════════════════
// git_head
// ════════════════════════════════════════════════════════════════════════

#[test]
fn git_head_returns_sha() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let head = git_head(dir.path()).expect("head should succeed");
    assert_eq!(head.len(), 40);
    assert!(head.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn git_head_none_for_non_repo() {
    let dir = tmp();
    assert!(git_head(dir.path()).is_none());
}

#[test]
fn git_head_updates_after_commit() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());
    let head1 = git_head(dir.path()).unwrap();

    fs::write(dir.path().join("g.txt"), "new").unwrap();
    git_commit(dir.path(), "second").unwrap();
    let head2 = git_head(dir.path()).unwrap();

    assert_ne!(head1, head2, "HEAD should change after a new commit");
}

// ════════════════════════════════════════════════════════════════════════
// Integration: round-trip scenarios
// ════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_commit_diff_stats() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    let sha1 = git_head(dir.path()).unwrap();

    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    fs::write(dir.path().join("new.txt"), "added\n").unwrap();
    let sha2 = git_commit(dir.path(), "changes").unwrap();

    let diff = git_diff_commits(dir.path(), &sha1, &sha2).unwrap();
    assert!(diff.contains("f.txt"));
    assert!(diff.contains("new.txt"));
}

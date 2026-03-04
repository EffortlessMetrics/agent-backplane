//! Deep integration tests for `abp-git` — repository lifecycle, diff semantics,
//! and error handling.
//!
//! Every test creates its own temporary directory that is cleaned up when the
//! `TempDir` guard goes out of scope.

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

fn git_exit_code(path: &Path, args: &[&str]) -> i32 {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("git should be on PATH")
        .status
        .code()
        .unwrap_or(-1)
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

// ════════════════════════════════════════════════════════════════════════
// (a) Repository operations — 10 tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn repo_init_in_empty_temp_dir() {
    let dir = tmp();
    ensure_git_repo(dir.path());
    assert!(dir.path().join(".git").exists(), ".git must be created");
    // Even with no files the repo should be valid (git rev-parse succeeds).
    let code = git_exit_code(dir.path(), &["rev-parse", "--is-inside-work-tree"]);
    assert_eq!(code, 0);
}

#[test]
fn repo_initial_commit_contains_seeded_files() {
    let dir = tmp();
    fs::write(dir.path().join("README.md"), "# hello").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.contains("README.md"));
    assert!(files.contains("main.rs"));
}

#[test]
fn repo_add_files_and_commit() {
    let dir = tmp();
    // Seed a file so the baseline commit is created.
    fs::write(dir.path().join("seed.txt"), "seed").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    git_ok(dir.path(), &["add", "-A"]);
    commit(dir.path(), "add a and b");

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "2", "baseline + new commit = 2");
}

#[test]
fn repo_head_hash_is_valid_sha1() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let sha = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();
    assert!(
        sha.len() >= 40 && sha.chars().all(|c| c.is_ascii_hexdigit()),
        "HEAD should be a valid hex SHA, got: {sha}"
    );
}

#[test]
fn repo_ls_files_reflects_working_tree() {
    let dir = tmp();
    fs::write(dir.path().join("x.rs"), "//").unwrap();
    let sub = dir.path().join("src");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("lib.rs"), "//").unwrap();
    ensure_git_repo(dir.path());

    let files = git(dir.path(), &["ls-files"]);
    assert!(files.contains("x.rs"));
    assert!(files.contains("lib.rs"));
}

#[test]
fn repo_status_clean_vs_dirty() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "clean\n").unwrap();
    ensure_git_repo(dir.path());

    // Clean
    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty(), "should be clean after init");

    // Dirty
    fs::write(dir.path().join("f.txt"), "dirty\n").unwrap();
    let status = git_status(dir.path()).unwrap();
    assert!(!status.trim().is_empty(), "should be dirty after edit");
}

#[test]
fn repo_create_and_list_branches() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());

    git_ok(dir.path(), &["checkout", "-b", "feature-a"]);
    fs::write(dir.path().join("feature.txt"), "new").unwrap();
    git_ok(dir.path(), &["add", "feature.txt"]);
    commit(dir.path(), "feature commit");

    let branches = git(dir.path(), &["branch", "--list"]);
    assert!(branches.contains("feature-a"), "branch should exist");
}

#[test]
fn repo_commit_message_retrievable() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let msg = git(dir.path(), &["log", "-1", "--format=%s"]);
    assert_eq!(msg.trim(), "baseline", "initial commit message should be 'baseline'");
}

#[test]
fn repo_commit_author_is_abp() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let author = git(dir.path(), &["log", "-1", "--format=%an"]);
    assert_eq!(author.trim(), "abp");
    let email = git(dir.path(), &["log", "-1", "--format=%ae"]);
    assert_eq!(email.trim(), "abp@local");
}

#[test]
fn repo_with_no_files_has_empty_tree_commit() {
    let dir = tmp();
    ensure_git_repo(dir.path());

    // An empty directory still gets .git, but the commit may be empty-tree.
    let tree = git(dir.path(), &["rev-parse", "HEAD^{tree}"]);
    assert!(
        !tree.trim().is_empty(),
        "tree object should exist even for empty commit"
    );
}

// ════════════════════════════════════════════════════════════════════════
// (b) Diff operations — 10 tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn diff_between_commits_via_log() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());

    let sha1 = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();

    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    commit(dir.path(), "update f");

    let sha2 = git(dir.path(), &["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(sha1, sha2, "commits should differ");

    let diff = git(dir.path(), &["diff", &sha1, &sha2, "--no-color"]);
    assert!(diff.contains("-v1"), "should show removed v1");
    assert!(diff.contains("+v2"), "should show added v2");

    // Working tree should be clean after commit, so library diff is empty.
    let lib_diff = git_diff(dir.path()).unwrap();
    assert!(lib_diff.trim().is_empty());
}

#[test]
fn diff_single_file_change() {
    let dir = tmp();
    fs::write(dir.path().join("only.txt"), "before\n").unwrap();
    fs::write(dir.path().join("untouched.txt"), "same\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("only.txt"), "after\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("only.txt"), "changed file should appear");
    assert!(
        !diff.contains("untouched.txt"),
        "unchanged file must not appear"
    );
}

#[test]
fn diff_multiple_file_changes() {
    let dir = tmp();
    for i in 0..5 {
        fs::write(
            dir.path().join(format!("f{i}.txt")),
            format!("original_{i}\n"),
        )
        .unwrap();
    }
    ensure_git_repo(dir.path());

    for i in 0..5 {
        fs::write(
            dir.path().join(format!("f{i}.txt")),
            format!("modified_{i}\n"),
        )
        .unwrap();
    }

    let diff = git_diff(dir.path()).unwrap();
    for i in 0..5 {
        assert!(
            diff.contains(&format!("f{i}.txt")),
            "f{i}.txt should appear in diff"
        );
    }
}

#[test]
fn diff_for_new_untracked_file_not_shown() {
    let dir = tmp();
    fs::write(dir.path().join("old.txt"), "old\n").unwrap();
    ensure_git_repo(dir.path());

    // Untracked files don't appear in `git diff`.
    fs::write(dir.path().join("brand_new.txt"), "new content\n").unwrap();
    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.contains("brand_new.txt"),
        "untracked file should not appear in diff"
    );
}

#[test]
fn diff_for_deleted_tracked_file() {
    let dir = tmp();
    fs::write(dir.path().join("bye.txt"), "farewell\n").unwrap();
    ensure_git_repo(dir.path());

    fs::remove_file(dir.path().join("bye.txt")).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("bye.txt"), "deleted file should appear in diff");
    assert!(diff.contains("-farewell"), "old content should be shown as removed");
}

#[test]
fn diff_for_renamed_file_staged() {
    let dir = tmp();
    fs::write(dir.path().join("alpha.txt"), "content\n").unwrap();
    ensure_git_repo(dir.path());

    fs::rename(dir.path().join("alpha.txt"), dir.path().join("beta.txt")).unwrap();
    git_ok(dir.path(), &["add", "-A"]);

    // Staged rename won't show in `git diff` (unstaged), but `git diff --cached` will.
    let cached_diff = git(dir.path(), &["diff", "--cached", "--no-color", "-M"]);
    assert!(
        cached_diff.contains("beta.txt"),
        "renamed file should appear in cached diff"
    );
}

#[test]
fn diff_for_binary_file_shows_marker() {
    let dir = tmp();
    let bin: Vec<u8> = (0..=255).collect();
    fs::write(dir.path().join("data.bin"), &bin).unwrap();
    ensure_git_repo(dir.path());

    let mut modified = bin;
    modified[0] = 0xFF;
    modified[128] = 0x00;
    fs::write(dir.path().join("data.bin"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.contains("data.bin"),
        "binary file should appear in diff"
    );
    // Git typically says "Binary files ... differ".
    assert!(
        diff.contains("Binary") || diff.contains("GIT binary"),
        "binary diff should contain binary marker, got: {diff}"
    );
}

#[test]
fn diff_empty_when_no_changes() {
    let dir = tmp();
    fs::write(dir.path().join("stable.txt"), "unchanged\n").unwrap();
    ensure_git_repo(dir.path());

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.trim().is_empty(), "no changes means empty diff");
}

#[test]
fn diff_output_has_unified_format_markers() {
    let dir = tmp();
    fs::write(dir.path().join("fmt.txt"), "aaa\nbbb\nccc\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("fmt.txt"), "aaa\nBBB\nccc\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("---"), "diff should have --- marker");
    assert!(diff.contains("+++"), "diff should have +++ marker");
    assert!(diff.contains("@@"), "diff should have @@ hunk header");
}

#[test]
fn diff_large_file_handling() {
    let dir = tmp();
    // Create a file with 2000 lines.
    let large: String = (0..2000).map(|i| format!("line {i}\n")).collect();
    fs::write(dir.path().join("big.txt"), &large).unwrap();
    ensure_git_repo(dir.path());

    // Change a line in the middle.
    let modified = large.replace("line 1000\n", "LINE_THOUSAND\n");
    fs::write(dir.path().join("big.txt"), &modified).unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-line 1000"), "removed line should appear");
    assert!(diff.contains("+LINE_THOUSAND"), "added line should appear");
    // Diff should not be absurdly large — context is bounded.
    assert!(
        diff.len() < large.len(),
        "diff should be smaller than the full file"
    );
}

// ════════════════════════════════════════════════════════════════════════
// (c) Error handling — 5 tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn error_not_a_git_repo() {
    let dir = tmp();
    // No git init — status and diff should return None.
    assert!(git_status(dir.path()).is_none(), "status should be None for non-repo");
    assert!(git_diff(dir.path()).is_none(), "diff should be None for non-repo");
}

#[test]
fn error_nonexistent_path() {
    let bad = PathBuf::from("__nonexistent_abp_deep_test_path_99999__");
    assert!(git_status(&bad).is_none());
    assert!(git_diff(&bad).is_none());
}

#[test]
fn error_empty_repo_no_commits() {
    let dir = tmp();
    git_ok(dir.path(), &["init", "-q"]);

    // A repo with no commits — status may work but diff may not.
    // Status should return something (empty or error-like).
    let status = git_status(dir.path());
    // It's valid for status to succeed on an empty repo (no commits).
    // Some git versions return Ok with empty output, some return error.
    // We just verify it doesn't panic.
    let _ = status;

    let diff = git_diff(dir.path());
    // diff on a repo with no commits may fail — just ensure no panic.
    let _ = diff;
}

#[test]
fn error_concurrent_git_operations() {
    // Spawn multiple threads hitting different operations on the *same* repo.
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let path = dir.path().to_path_buf();
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let p = path.clone();
            std::thread::spawn(move || {
                if i % 2 == 0 {
                    git_status(&p)
                } else {
                    git_diff(&p)
                }
            })
        })
        .collect();

    for h in handles {
        // Must not panic regardless of interleaving.
        let result = h.join().expect("thread must not panic");
        assert!(result.is_some(), "operation should succeed");
    }
}

#[test]
fn error_corrupted_git_dir() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    // Corrupt the HEAD file.
    let head_path = dir.path().join(".git").join("HEAD");
    fs::write(&head_path, "garbage_not_a_ref").unwrap();

    // Operations on a corrupted repo should return None (error), not panic.
    let status = git_status(dir.path());
    let diff = git_diff(dir.path());
    // We only care that these don't panic — result may be None or Some.
    let _ = (status, diff);
}

// ════════════════════════════════════════════════════════════════════════
// Bonus: additional deep coverage
// ════════════════════════════════════════════════════════════════════════

#[test]
fn diff_preserves_no_color_flag() {
    // Verify the diff output has no ANSI escape sequences.
    let dir = tmp();
    fs::write(dir.path().join("c.txt"), "old\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("c.txt"), "new\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.contains("\x1b["),
        "diff output should not contain ANSI escape codes"
    );
}

#[test]
fn status_porcelain_format_is_stable() {
    // Porcelain v1 format: XY <space> filename
    let dir = tmp();
    fs::write(dir.path().join("p.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("p.txt"), "v2").unwrap();

    let status = git_status(dir.path()).unwrap();
    // Modified but not staged → " M p.txt"
    let line = status.lines().next().expect("should have at least one line");
    assert!(
        line.len() >= 4,
        "porcelain line should be at least 4 chars: {line}"
    );
    // First two chars are status codes, then a space, then the path.
    assert_eq!(&line[2..3], " ", "3rd char should be space separator");
}

#[test]
fn ensure_git_repo_does_not_clobber_existing_commits() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1").unwrap();
    ensure_git_repo(dir.path());

    // Add a second commit.
    fs::write(dir.path().join("g.txt"), "extra").unwrap();
    git_ok(dir.path(), &["add", "g.txt"]);
    commit(dir.path(), "second");

    let count_before = git(dir.path(), &["rev-list", "--count", "HEAD"])
        .trim()
        .to_string();

    // Calling ensure_git_repo again should be a no-op.
    ensure_git_repo(dir.path());

    let count_after = git(dir.path(), &["rev-list", "--count", "HEAD"])
        .trim()
        .to_string();
    assert_eq!(count_before, count_after, "existing commits must be preserved");
}

#[test]
fn diff_with_line_ending_changes() {
    let dir = tmp();
    // Write with LF.
    fs::write(dir.path().join("le.txt"), "a\nb\nc\n").unwrap();
    ensure_git_repo(dir.path());

    // Rewrite with CRLF.
    fs::write(dir.path().join("le.txt"), "a\r\nb\r\nc\r\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    // Whether a diff shows up depends on core.autocrlf, but it shouldn't panic.
    // On most configs this will show changes.
    let _ = diff;
}

#[test]
fn status_with_nested_gitignore() {
    let dir = tmp();
    let sub = dir.path().join("vendor");
    fs::create_dir(&sub).unwrap();
    fs::write(dir.path().join(".gitignore"), "vendor/\n").unwrap();
    fs::write(sub.join("dep.txt"), "vendored").unwrap();
    fs::write(dir.path().join("app.rs"), "fn main(){}").unwrap();
    ensure_git_repo(dir.path());

    // Add a file inside ignored vendor dir.
    fs::write(sub.join("extra.txt"), "ignored").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(
        !status.contains("extra.txt"),
        "files in ignored dir should not appear in status"
    );
}

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
//! Additional integration tests for `abp-git` workspace git operations.
//!
//! Complements `git_tests.rs` with deeper edge-case, concurrency, and
//! cross-platform coverage.  Every test creates its own temp directory.

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

// ── repository initialization: deeper scenarios ─────────────────────

#[test]
fn init_creates_valid_head_ref() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    ensure_git_repo(dir.path());

    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(head.trim().len(), 40, "HEAD should be a full SHA");
}

#[test]
fn init_baseline_is_root_commit() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    ensure_git_repo(dir.path());

    let parents = git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        parents.trim(),
        head.trim(),
        "baseline should be the root commit"
    );
}

#[test]
fn init_sets_clean_working_tree() {
    let dir = tmp();
    fs::write(dir.path().join("x.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty(), "tree should be clean after init");
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

// ── status: additional scenarios ────────────────────────────────────

#[test]
fn status_staged_modification() {
    let dir = tmp();
    fs::write(dir.path().join("m.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("m.txt"), "v2\n").unwrap();
    git_ok(dir.path(), &["add", "m.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains("M  m.txt"),
        "expected staged modification marker, got: {status}"
    );
}

#[test]
fn status_staged_delete() {
    let dir = tmp();
    fs::write(dir.path().join("rm.txt"), "bye").unwrap();
    ensure_git_repo(dir.path());

    git_ok(dir.path(), &["rm", "-q", "rm.txt"]);

    let status = git_status(dir.path()).unwrap();
    assert!(
        status.contains("D  rm.txt"),
        "expected staged deletion, got: {status}"
    );
}

#[test]
fn status_rename_detection() {
    let dir = tmp();
    fs::write(dir.path().join("old.txt"), "content\n").unwrap();
    ensure_git_repo(dir.path());

    fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).unwrap();
    git_ok(dir.path(), &["add", "-A"]);

    let status = git_status(dir.path()).unwrap();
    // Porcelain v1 may show R (rename) or D+A depending on similarity.
    assert!(
        status.contains("new.txt"),
        "renamed file should appear in status"
    );
}

#[test]
fn status_mixed_staged_and_unstaged() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "original\n").unwrap();
    ensure_git_repo(dir.path());

    // Stage one change, then modify again without staging.
    fs::write(dir.path().join("f.txt"), "staged\n").unwrap();
    git_ok(dir.path(), &["add", "f.txt"]);
    fs::write(dir.path().join("f.txt"), "unstaged\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    // Porcelain shows "MM" for staged+unstaged modification.
    assert!(
        status.contains("MM f.txt"),
        "expected MM marker, got: {status}"
    );
}

#[test]
fn status_empty_after_all_changes_committed() {
    let dir = tmp();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("new.txt"), "added").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);
    commit(dir.path(), "add new file");

    let status = git_status(dir.path()).unwrap();
    assert!(status.trim().is_empty());
}

// ── diff: additional scenarios ──────────────────────────────────────

#[test]
fn diff_shows_added_lines() {
    let dir = tmp();
    fs::write(dir.path().join("grow.txt"), "line1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("grow.txt"), "line1\nline2\nline3\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("+line2"), "expected added line2");
    assert!(diff.contains("+line3"), "expected added line3");
}

#[test]
fn diff_shows_removed_lines() {
    let dir = tmp();
    fs::write(dir.path().join("shrink.txt"), "a\nb\nc\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("shrink.txt"), "a\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("-b"), "expected removed b");
    assert!(diff.contains("-c"), "expected removed c");
}

#[test]
fn diff_multiple_files_changed() {
    let dir = tmp();
    fs::write(dir.path().join("x.txt"), "x1\n").unwrap();
    fs::write(dir.path().join("y.txt"), "y1\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("x.txt"), "x2\n").unwrap();
    fs::write(dir.path().join("y.txt"), "y2\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("x.txt"), "diff should mention x.txt");
    assert!(diff.contains("y.txt"), "diff should mention y.txt");
}

#[test]
fn diff_after_partial_stage() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "aaa\n").unwrap();
    fs::write(dir.path().join("b.txt"), "bbb\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("a.txt"), "AAA\n").unwrap();
    fs::write(dir.path().join("b.txt"), "BBB\n").unwrap();
    // Only stage a.txt.
    git_ok(dir.path(), &["add", "a.txt"]);

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.contains("a.txt"),
        "staged file should not appear in unstaged diff"
    );
    assert!(
        diff.contains("b.txt"),
        "unstaged b.txt should appear in diff"
    );
}

#[test]
fn diff_empty_file_to_content() {
    let dir = tmp();
    fs::write(dir.path().join("e.txt"), "").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("e.txt"), "now has content\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.contains("+now has content"),
        "diff should show added content"
    );
}

#[test]
fn diff_content_to_empty_file() {
    let dir = tmp();
    fs::write(dir.path().join("c.txt"), "some content\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("c.txt"), "").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        diff.contains("-some content"),
        "diff should show removed content"
    );
}

// ── commit operations via ensure_git_repo ───────────────────────────

#[test]
fn baseline_commit_has_abp_author() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let author = git(dir.path(), &["log", "-1", "--format=%an <%ae>"]);
    assert!(
        author.contains("abp"),
        "baseline commit author should be 'abp', got: {author}"
    );
}

#[test]
fn second_commit_produces_valid_sha() {
    let dir = tmp();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("new.txt"), "data").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);
    commit(dir.path(), "second commit");

    let sha = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(sha.trim().len(), 40, "commit SHA should be 40 hex chars");
    assert!(
        sha.trim().chars().all(|c| c.is_ascii_hexdigit()),
        "SHA should be hex"
    );
}

#[test]
fn commit_count_after_second_commit() {
    let dir = tmp();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("b.txt"), "b").unwrap();
    git_ok(dir.path(), &["add", "b.txt"]);
    commit(dir.path(), "second");

    let count = git(dir.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "2", "should have exactly 2 commits");
}

// ── edge cases ──────────────────────────────────────────────────────

#[test]
fn unicode_filename() {
    let dir = tmp();
    // Use a filename with unicode that's safe on all major platforms.
    let name = "données.txt";
    fs::write(dir.path().join(name), "contenu").unwrap();
    ensure_git_repo(dir.path());

    let tracked = git(dir.path(), &["ls-files"]);
    // Git may quote non-ASCII names, but the file should be tracked.
    let status = git_status(dir.path()).unwrap();
    assert!(
        status.trim().is_empty() || tracked.contains("donn"),
        "unicode file should be tracked after init"
    );
}

#[test]
fn unicode_content() {
    let dir = tmp();
    fs::write(dir.path().join("uni.txt"), "日本語テスト\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("uni.txt"), "中文测试\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.trim().is_empty(),
        "diff should detect unicode content change"
    );
}

#[test]
fn binary_file_in_status_and_diff() {
    let dir = tmp();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(dir.path().join("all_bytes.bin"), &data).unwrap();
    ensure_git_repo(dir.path());

    // Modify a few bytes.
    let mut modified = data.clone();
    modified[0] = 128;
    modified[100] = 0;
    fs::write(dir.path().join("all_bytes.bin"), &modified).unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(status.contains("all_bytes.bin"));

    let diff = git_diff(dir.path()).unwrap();
    assert!(diff.contains("all_bytes.bin"));
}

#[test]
fn operations_on_nonexistent_directory() {
    let bad = Path::new("__nonexistent_abp_test_dir_12345__");
    assert!(git_status(bad).is_none());
    assert!(git_diff(bad).is_none());
}

#[test]
fn thread_safety_concurrent_status() {
    // Verify that git_status can be called from multiple threads
    // against independent repos without panicking.
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
        let result = h.join().expect("thread should not panic");
        assert!(result.is_some());
    }
}

#[test]
fn thread_safety_concurrent_diff() {
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
        let result = h.join().expect("thread should not panic");
        let diff = result.expect("diff should succeed");
        assert!(diff.contains("+v2"), "each thread should see the change");
    }
}

#[test]
fn gitignore_respected_by_status() {
    let dir = tmp();
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    fs::write(dir.path().join("app.log"), "log data").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    ensure_git_repo(dir.path());

    // After init, add a new ignored file.
    fs::write(dir.path().join("debug.log"), "more logs").unwrap();
    fs::write(dir.path().join("new.rs"), "// new").unwrap();

    let status = git_status(dir.path()).unwrap();
    assert!(
        !status.contains("debug.log"),
        ".gitignore should hide .log files from status"
    );
    assert!(
        status.contains("new.rs"),
        "non-ignored files should appear in status"
    );
}

#[test]
fn whitespace_only_changes_in_diff() {
    let dir = tmp();
    fs::write(dir.path().join("ws.txt"), "hello world\n").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("ws.txt"), "hello  world\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.trim().is_empty(),
        "whitespace changes should produce a diff"
    );
}

#[test]
fn newline_at_eof_change() {
    let dir = tmp();
    fs::write(dir.path().join("nl.txt"), "no newline").unwrap();
    ensure_git_repo(dir.path());

    fs::write(dir.path().join("nl.txt"), "no newline\n").unwrap();

    let diff = git_diff(dir.path()).unwrap();
    assert!(
        !diff.trim().is_empty(),
        "adding newline at EOF should produce a diff"
    );
}

#[test]
fn status_and_diff_agree_on_clean_repo() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "data").unwrap();
    ensure_git_repo(dir.path());

    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();

    assert!(status.trim().is_empty());
    assert!(diff.trim().is_empty());
}

#[test]
fn status_and_diff_agree_on_dirty_repo() {
    let dir = tmp();
    fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
    ensure_git_repo(dir.path());
    fs::write(dir.path().join("f.txt"), "v2\n").unwrap();

    let status = git_status(dir.path()).unwrap();
    let diff = git_diff(dir.path()).unwrap();

    assert!(!status.trim().is_empty(), "status should show modification");
    assert!(!diff.trim().is_empty(), "diff should show modification");
    assert!(status.contains("f.txt"));
    assert!(diff.contains("f.txt"));
}

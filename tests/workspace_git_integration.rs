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
//! Deep integration tests for workspace staging, git initialization, and diff
//! extraction.  Focuses on the interplay between `WorkspaceStager`,
//! `WorkspaceManager`, `abp-git` helpers, and the `diff_workspace` analyser.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{diff_workspace, DiffSummary};
use abp_workspace::snapshot::{capture, compare};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

// ── helpers ─────────────────────────────────────────────────────────────────

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn git_ok(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git should be on PATH");
    assert!(st.success(), "git {args:?} failed");
}

/// Create a temp directory with a handful of seeded files.
fn make_source(files: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    for (rel, content) in files {
        let p = tmp.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
    }
    tmp
}

fn stage(src: &Path) -> abp_workspace::PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .stage()
        .expect("stage should succeed")
}

// ═══════════════════════════════════════════════════════════════════════════
//  1.  Git Initialization (10+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn git_head_ref_points_to_valid_branch() {
    let src = make_source(&[("a.txt", "hello")]);
    let ws = stage(src.path());
    let head = git(ws.path(), &["symbolic-ref", "HEAD"]);
    assert!(
        head.trim().starts_with("refs/heads/"),
        "HEAD should reference a branch, got: {head}"
    );
}

#[test]
fn git_rev_parse_head_succeeds() {
    let src = make_source(&[("a.txt", "hi")]);
    let ws = stage(src.path());
    let sha = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(sha.trim().len(), 40, "HEAD SHA should be 40 hex chars");
}

#[test]
fn git_objects_directory_exists() {
    let src = make_source(&[("a.txt", "data")]);
    let ws = stage(src.path());
    assert!(ws.path().join(".git").join("objects").is_dir());
}

#[test]
fn git_refs_directory_exists() {
    let src = make_source(&[("a.txt", "data")]);
    let ws = stage(src.path());
    assert!(ws.path().join(".git").join("refs").is_dir());
}

#[test]
fn git_log_count_is_one() {
    let src = make_source(&[("x.txt", "x")]);
    let ws = stage(src.path());
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1", "expected exactly one commit");
}

#[test]
fn git_baseline_author_is_abp() {
    let src = make_source(&[("x.txt", "data")]);
    let ws = stage(src.path());
    let author = git(ws.path(), &["log", "-1", "--format=%an"]);
    assert_eq!(author.trim(), "abp");
}

#[test]
fn git_baseline_email_is_abp_local() {
    let src = make_source(&[("x.txt", "data")]);
    let ws = stage(src.path());
    let email = git(ws.path(), &["log", "-1", "--format=%ae"]);
    assert_eq!(email.trim(), "abp@local");
}

#[test]
fn git_fsck_reports_no_errors() {
    let src = make_source(&[("a.txt", "ok")]);
    let ws = stage(src.path());
    git_ok(ws.path(), &["fsck", "--no-dangling"]);
}

#[test]
fn git_index_is_clean_after_init() {
    let src = make_source(&[("a.txt", "content"), ("b.txt", "other")]);
    let ws = stage(src.path());
    let status = git(ws.path(), &["status", "--porcelain"]);
    assert!(
        status.trim().is_empty(),
        "expected clean index, got: {status}"
    );
}

#[test]
fn git_ls_tree_matches_staged_files() {
    let src = make_source(&[("a.txt", "aaa"), ("sub/b.txt", "bbb")]);
    let ws = stage(src.path());
    let tree = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    let files: Vec<&str> = tree
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert!(files.contains(&"a.txt"));
    assert!(files.contains(&"sub/b.txt"));
    assert_eq!(files.len(), 2);
}

#[test]
fn git_show_baseline_commit_message() {
    let src = make_source(&[("f.txt", "x")]);
    let ws = stage(src.path());
    let msg = git(ws.path(), &["log", "-1", "--format=%s"]);
    assert_eq!(msg.trim(), "baseline");
}

#[test]
fn git_init_with_empty_source_still_creates_repo() {
    let src = TempDir::new().unwrap();
    let ws = stage(src.path());
    assert!(ws.path().join(".git").exists());
}

// ═══════════════════════════════════════════════════════════════════════════
//  2.  Diff Extraction (10+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_workspace_empty_when_no_changes() {
    let src = make_source(&[("a.txt", "hello")]);
    let ws = stage(src.path());
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

#[test]
fn diff_workspace_detects_single_file_addition() {
    let src = make_source(&[("a.txt", "base")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("new.txt"), "fresh\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 1);
    assert!(summary
        .added
        .iter()
        .any(|p| p.to_string_lossy().contains("new.txt")));
}

#[test]
fn diff_workspace_detects_single_file_modification() {
    let src = make_source(&[("a.txt", "original\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("a.txt"), "modified\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 1);
    assert!(summary
        .modified
        .iter()
        .any(|p| p.to_string_lossy().contains("a.txt")));
}

#[test]
fn diff_workspace_detects_single_file_deletion() {
    let src = make_source(&[("a.txt", "bye\n")]);
    let ws = stage(src.path());
    fs::remove_file(ws.path().join("a.txt")).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.deleted.len(), 1);
}

#[test]
fn diff_workspace_counts_added_lines() {
    let src = make_source(&[("a.txt", "line1\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("b.txt"), "one\ntwo\nthree\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.total_additions, 3);
}

#[test]
fn diff_workspace_counts_deleted_lines() {
    let src = make_source(&[("a.txt", "one\ntwo\nthree\n")]);
    let ws = stage(src.path());
    fs::remove_file(ws.path().join("a.txt")).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.total_deletions, 3);
}

#[test]
fn diff_workspace_modification_counts_both_add_and_del() {
    let src = make_source(&[("a.txt", "old\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("a.txt"), "new\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_additions >= 1);
    assert!(summary.total_deletions >= 1);
}

#[test]
fn diff_workspace_binary_addition_shows_in_added() {
    let src = make_source(&[("a.txt", "base")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("bin.dat"), [0u8, 1, 2, 255, 0, 128]).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary
        .added
        .iter()
        .any(|p| p.to_string_lossy().contains("bin.dat")));
}

#[test]
fn diff_workspace_special_characters_in_content() {
    let src = make_source(&[("a.txt", "line α\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("a.txt"), "line β γ δ\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 1);
    assert!(summary.total_changes() >= 2);
}

#[test]
fn diff_workspace_multiple_operations_combined() {
    let src = make_source(&[
        ("keep.txt", "keep\n"),
        ("del.txt", "delete\n"),
        ("mod.txt", "old\n"),
    ]);
    let ws = stage(src.path());
    fs::write(ws.path().join("add.txt"), "new\n").unwrap();
    fs::remove_file(ws.path().join("del.txt")).unwrap();
    fs::write(ws.path().join("mod.txt"), "new content\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 1);
    assert_eq!(summary.deleted.len(), 1);
    assert_eq!(summary.modified.len(), 1);
    assert_eq!(summary.file_count(), 3);
}

#[test]
fn diff_workspace_file_in_subdirectory() {
    let src = make_source(&[("sub/deep/file.txt", "original\n")]);
    let ws = stage(src.path());
    fs::write(
        ws.path().join("sub").join("deep").join("file.txt"),
        "changed\n",
    )
    .unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 1);
}

#[test]
fn diff_workspace_total_changes_equals_sum() {
    let src = make_source(&[("a.txt", "one\ntwo\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("a.txt"), "one\nTWO\nthree\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(
        summary.total_changes(),
        summary.total_additions + summary.total_deletions
    );
}

#[test]
fn diff_summary_serde_roundtrip() {
    let summary = DiffSummary {
        added: vec!["new.txt".into()],
        modified: vec!["mod.txt".into()],
        deleted: vec!["del.txt".into()],
        total_additions: 10,
        total_deletions: 5,
    };
    let json = serde_json::to_string(&summary).unwrap();
    let deser: DiffSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(summary, deser);
}

// ═══════════════════════════════════════════════════════════════════════════
//  3.  Staged Workspace Behavior (10+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn staged_source_with_existing_gitignore_preserves_it() {
    let src = make_source(&[("a.txt", "hi"), (".gitignore", "*.log\n")]);
    let ws = stage(src.path());
    let content = fs::read_to_string(ws.path().join(".gitignore")).unwrap();
    assert_eq!(content, "*.log\n");
}

#[test]
fn staged_workspace_git_does_not_inherit_source_history() {
    let src = make_source(&[("a.txt", "v1")]);
    // Make the source a real git repo with 2 commits.
    git_ok(src.path(), &["init", "-q"]);
    git_ok(src.path(), &["add", "-A"]);
    git_ok(
        src.path(),
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@x",
            "commit",
            "-qm",
            "first",
        ],
    );
    fs::write(src.path().join("a.txt"), "v2").unwrap();
    git_ok(src.path(), &["add", "-A"]);
    git_ok(
        src.path(),
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@x",
            "commit",
            "-qm",
            "second",
        ],
    );

    let ws = stage(src.path());
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(
        count.trim(),
        "1",
        "staged workspace should have only one commit"
    );
}

#[test]
fn include_pattern_limits_git_tracked_files() {
    let src = make_source(&[("a.rs", "fn main(){}"), ("b.txt", "readme")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();
    let tree = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    let files: Vec<&str> = tree
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert!(files.contains(&"a.rs"));
    assert!(!files.contains(&"b.txt"));
}

#[test]
fn exclude_pattern_removes_from_git_tracked_files() {
    let src = make_source(&[("a.rs", "fn main(){}"), ("data.log", "log line")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .stage()
        .unwrap();
    let tree = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(!tree.contains("data.log"));
    assert!(tree.contains("a.rs"));
}

#[test]
fn combined_include_exclude_in_git() {
    let src = make_source(&[
        ("src/lib.rs", "code"),
        ("src/test.rs", "test"),
        ("README.md", "docs"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["**/test.rs".into()])
        .stage()
        .unwrap();
    let tree = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("src/lib.rs"));
    assert!(!tree.contains("test.rs"));
    assert!(!tree.contains("README.md"));
}

#[test]
fn staged_workspace_crlf_content_preserved() {
    let src = make_source(&[("win.txt", "line1\r\nline2\r\n")]);
    let ws = stage(src.path());
    let content = fs::read(ws.path().join("win.txt")).unwrap();
    assert!(
        content.windows(2).any(|w| w == b"\r\n"),
        "CRLF should be preserved"
    );
}

#[test]
fn staged_workspace_file_with_null_bytes() {
    let src = TempDir::new().unwrap();
    fs::write(src.path().join("nulls.bin"), [0u8; 64]).unwrap();
    let ws = stage(src.path());
    let content = fs::read(ws.path().join("nulls.bin")).unwrap();
    assert_eq!(content.len(), 64);
    assert!(content.iter().all(|&b| b == 0));
}

#[test]
fn staged_workspace_nested_dirs_all_tracked() {
    let src = make_source(&[
        ("a/b/c/d.txt", "deep"),
        ("a/b/e.txt", "mid"),
        ("a/f.txt", "shallow"),
    ]);
    let ws = stage(src.path());
    let tree = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    let files: Vec<&str> = tree
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(files.len(), 3);
}

#[test]
fn staged_workspace_no_git_init_has_no_dot_git() {
    let src = make_source(&[("a.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn staged_workspace_no_git_init_still_copies_files() {
    let src = make_source(&[("a.txt", "hi"), ("b.txt", "bye")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("a.txt").exists());
    assert!(ws.path().join("b.txt").exists());
}

#[test]
fn staged_workspace_manager_prepare_creates_git() {
    let src = make_source(&[("a.txt", "x")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join(".git").exists());
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1");
}

// ═══════════════════════════════════════════════════════════════════════════
//  4.  Workspace Cleanup & Lifecycle (10+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_temp_dir_cleaned_on_drop() {
    let src = make_source(&[("a.txt", "data")]);
    let path;
    {
        let ws = stage(src.path());
        path = ws.path().to_path_buf();
        assert!(path.exists());
    }
    assert!(!path.exists(), "temp dir should be removed after drop");
}

#[test]
fn two_workspaces_have_different_paths() {
    let src = make_source(&[("a.txt", "data")]);
    let ws1 = stage(src.path());
    let ws2 = stage(src.path());
    assert_ne!(ws1.path(), ws2.path());
}

#[test]
fn workspace_modification_does_not_bleed() {
    let src = make_source(&[("a.txt", "original")]);
    let ws1 = stage(src.path());
    let ws2 = stage(src.path());
    fs::write(ws1.path().join("a.txt"), "mutated").unwrap();
    let c2 = fs::read_to_string(ws2.path().join("a.txt")).unwrap();
    assert_eq!(c2, "original");
}

#[test]
fn dropping_one_workspace_preserves_another() {
    let src = make_source(&[("a.txt", "data")]);
    let ws1 = stage(src.path());
    let path2;
    {
        let ws2 = stage(src.path());
        path2 = ws2.path().to_path_buf();
    }
    // ws2 dropped, ws1 still alive
    assert!(ws1.path().exists());
    assert!(!path2.exists());
}

#[test]
fn workspace_path_is_a_directory() {
    let src = make_source(&[("a.txt", "data")]);
    let ws = stage(src.path());
    assert!(ws.path().is_dir());
}

#[test]
fn workspace_path_is_absolute() {
    let src = make_source(&[("a.txt", "data")]);
    let ws = stage(src.path());
    assert!(ws.path().is_absolute());
}

#[test]
fn passthrough_workspace_preserves_path() {
    let src = make_source(&[("a.txt", "data")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn passthrough_workspace_does_not_init_git() {
    let src = make_source(&[("a.txt", "data")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let _ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(!src.path().join(".git").exists());
}

#[test]
fn concurrent_three_workspaces_all_independent() {
    let src = make_source(&[("a.txt", "shared")]);
    let ws_vec: Vec<_> = (0..3).map(|_| stage(src.path())).collect();
    // Mutate each differently.
    for (i, ws) in ws_vec.iter().enumerate() {
        fs::write(ws.path().join("a.txt"), format!("version-{i}")).unwrap();
    }
    // Verify isolation.
    for (i, ws) in ws_vec.iter().enumerate() {
        let content = fs::read_to_string(ws.path().join("a.txt")).unwrap();
        assert_eq!(content, format!("version-{i}"));
    }
}

#[test]
fn stager_error_nonexistent_source() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
}

#[test]
fn stager_error_no_source_root() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
//  5.  Snapshot + Git Cross-Validation (additional)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_file_count_matches_git_ls_files() {
    let src = make_source(&[
        ("a.txt", "aaa"),
        ("b.rs", "fn main(){}"),
        ("sub/c.json", "{}"),
    ]);
    let ws = stage(src.path());
    let snap = capture(ws.path()).unwrap();
    let ls = git(ws.path(), &["ls-files"]);
    let git_count = ls.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(snap.file_count(), git_count);
}

#[test]
fn snapshot_after_mutation_differs_from_initial() {
    let src = make_source(&[("a.txt", "before\n")]);
    let ws = stage(src.path());
    let snap1 = capture(ws.path()).unwrap();
    fs::write(ws.path().join("a.txt"), "after\n").unwrap();
    let snap2 = capture(ws.path()).unwrap();
    let diff = compare(&snap1, &snap2);
    assert_eq!(diff.modified.len(), 1);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
}

#[test]
fn snapshot_and_diff_workspace_agree_on_additions() {
    let src = make_source(&[("base.txt", "base")]);
    let ws = stage(src.path());
    let snap1 = capture(ws.path()).unwrap();

    fs::write(ws.path().join("new1.txt"), "one\n").unwrap();
    fs::write(ws.path().join("new2.txt"), "two\n").unwrap();

    let snap2 = capture(ws.path()).unwrap();
    let snap_diff = compare(&snap1, &snap2);
    let git_diff = diff_workspace(&ws).unwrap();

    assert_eq!(snap_diff.added.len(), git_diff.added.len());
}

#[test]
fn diff_workspace_after_revert_is_empty() {
    let src = make_source(&[("a.txt", "original\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("a.txt"), "changed\n").unwrap();
    // Revert
    fs::write(ws.path().join("a.txt"), "original\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary.is_empty(),
        "reverted changes should produce empty diff"
    );
}

#[test]
fn diff_workspace_large_addition() {
    let src = make_source(&[("a.txt", "small")]);
    let ws = stage(src.path());
    let big = "x\n".repeat(500);
    fs::write(ws.path().join("big.txt"), &big).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.total_additions, 500);
}

#[test]
fn git_status_helper_agrees_with_diff_workspace_on_clean() {
    let src = make_source(&[("a.txt", "data")]);
    let ws = stage(src.path());
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(status.trim().is_empty());
    assert!(summary.is_empty());
}

#[test]
fn git_diff_helper_detects_modification() {
    let src = make_source(&[("a.txt", "old\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("a.txt"), "new\n").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("-old"), "diff should show removed line");
    assert!(diff.contains("+new"), "diff should show added line");
}

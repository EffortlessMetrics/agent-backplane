// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for workspace staging and git diff
//! functionality.
//!
//! Covers: workspace creation, git initialization, diff after changes, adding
//! new files, deleting files, .git exclusion, glob filtering, large workspaces,
//! nested directories, symlinks, binary files, cleanup, concurrent workspaces,
//! and path normalization.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{DiffAnalysis, DiffAnalyzer, DiffPolicy, WorkspaceDiff, diff_workspace};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog};
use abp_workspace::snapshot::{capture, compare};
use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};
use abp_workspace::{PreparedWorkspace, WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use walkdir::WalkDir;

// ═══════════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════════

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

fn stage(src: &Path) -> PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .stage()
        .expect("stage should succeed")
}

fn stage_with_globs(src: &Path, include: Vec<String>, exclude: Vec<String>) -> PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .include(include)
        .exclude(exclude)
        .stage()
        .expect("stage should succeed")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Collect sorted relative file paths (excluding `.git`) under `root`.
fn collect_files(root: &Path) -> Vec<String> {
    let mut files: Vec<String> = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == ".git"))
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            e.path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    files
}

fn staged_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  1. Workspace creation — temp dir created, files copied
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_creation_produces_temp_dir() {
    let src = make_source(&[("hello.txt", "world")]);
    let ws = stage(src.path());
    assert!(ws.path().exists(), "workspace dir must exist");
    assert_ne!(ws.path(), src.path(), "staged dir must differ from source");
}

#[test]
fn workspace_copies_single_file() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = stage(src.path());
    let copied = ws.path().join("file.txt");
    assert!(copied.exists());
    assert_eq!(fs::read_to_string(copied).unwrap(), "content");
}

#[test]
fn workspace_copies_multiple_files() {
    let src = make_source(&[("a.txt", "alpha"), ("b.txt", "beta"), ("c.txt", "gamma")]);
    let ws = stage(src.path());
    assert_eq!(collect_files(ws.path()), vec!["a.txt", "b.txt", "c.txt"]);
}

#[test]
fn workspace_preserves_file_contents() {
    let src = make_source(&[("data.txt", "exact content\nline2")]);
    let ws = stage(src.path());
    let actual = fs::read_to_string(ws.path().join("data.txt")).unwrap();
    assert_eq!(actual, "exact content\nline2");
}

#[test]
fn workspace_creation_via_manager_staged() {
    let src = make_source(&[("m.txt", "managed")]);
    let spec = staged_spec(src.path());
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("m.txt").exists());
}

#[test]
fn workspace_passthrough_returns_original_path() {
    let src = make_source(&[("p.txt", "pass")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src.path());
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. Git initialization — auto-init git repo with baseline commit
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn git_init_creates_dot_git_directory() {
    let src = make_source(&[("init.txt", "data")]);
    let ws = stage(src.path());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn git_init_has_baseline_commit() {
    let src = make_source(&[("base.txt", "baseline")]);
    let ws = stage(src.path());
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"), "should have baseline commit");
}

#[test]
fn git_head_is_valid_sha() {
    let src = make_source(&[("a.txt", "x")]);
    let ws = stage(src.path());
    let sha = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(sha.trim().len(), 40);
}

#[test]
fn git_status_clean_after_staging() {
    let src = make_source(&[("a.txt", "clean")]);
    let ws = stage(src.path());
    let status = git(ws.path(), &["status", "--porcelain"]);
    assert!(status.trim().is_empty(), "workspace should be clean");
}

#[test]
fn git_init_all_files_tracked() {
    let src = make_source(&[("x.txt", "x"), ("y.txt", "y")]);
    let ws = stage(src.path());
    let tracked = git(ws.path(), &["ls-files"]);
    assert!(tracked.contains("x.txt"));
    assert!(tracked.contains("y.txt"));
}

#[test]
fn git_init_respects_disabled_flag() {
    let src = make_source(&[("no_git.txt", "val")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(
        !ws.path().join(".git").exists(),
        ".git should not exist when git_init=false"
    );
}

#[test]
fn git_init_commit_author_is_abp() {
    let src = make_source(&[("auth.txt", "a")]);
    let ws = stage(src.path());
    let log = git(ws.path(), &["log", "--format=%an"]);
    assert_eq!(log.trim(), "abp");
}

#[test]
fn git_init_commit_email_is_abp_local() {
    let src = make_source(&[("em.txt", "e")]);
    let ws = stage(src.path());
    let log = git(ws.path(), &["log", "--format=%ae"]);
    assert_eq!(log.trim(), "abp@local");
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. Diff after changes — modify files → git diff shows changes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_detects_modified_file() {
    let src = make_source(&[("mod.txt", "original")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("mod.txt"), "modified").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 1);
    assert_eq!(summary.modified[0], PathBuf::from("mod.txt"));
}

#[test]
fn diff_counts_additions_after_append() {
    let src = make_source(&[("lines.txt", "line1\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("lines.txt"), "line1\nline2\nline3\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_additions >= 2, "expected >=2 additions");
}

#[test]
fn diff_counts_deletions_after_truncation() {
    let src = make_source(&[("lines.txt", "line1\nline2\nline3\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("lines.txt"), "line1\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_deletions >= 2, "expected >=2 deletions");
}

#[test]
fn diff_summary_is_empty_when_no_changes() {
    let src = make_source(&[("same.txt", "unchanged")]);
    let ws = stage(src.path());
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
}

#[test]
fn diff_file_count_matches_modified_count() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("a.txt"), "A").unwrap();
    fs::write(ws.path().join("b.txt"), "B").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.file_count(), 2);
}

#[test]
fn diff_total_changes_is_sum_of_add_and_del() {
    let src = make_source(&[("t.txt", "old\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("t.txt"), "new\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(
        summary.total_changes(),
        summary.total_additions + summary.total_deletions
    );
}

#[test]
fn diff_analyzer_detects_modified() {
    let src = make_source(&[("f.txt", "before")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("f.txt"), "after").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.has_changes());
}

#[test]
fn diff_analyzer_file_was_modified() {
    let src = make_source(&[("check.txt", "v1")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("check.txt"), "v2").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.file_was_modified(Path::new("check.txt")));
}

#[test]
fn diff_analyzer_changed_files_list() {
    let src = make_source(&[("a.txt", "1"), ("b.txt", "2")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("a.txt"), "changed").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let changed = analyzer.changed_files();
    assert!(changed.contains(&PathBuf::from("a.txt")));
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. Add new files — new files appear in diff
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_detects_new_file() {
    let src = make_source(&[("existing.txt", "hi")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("brand_new.txt"), "new content").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 1);
    assert_eq!(summary.added[0], PathBuf::from("brand_new.txt"));
}

#[test]
fn diff_detects_multiple_new_files() {
    let src = make_source(&[("seed.txt", "s")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("new1.txt"), "n1").unwrap();
    fs::write(ws.path().join("new2.txt"), "n2").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 2);
}

#[test]
fn diff_new_file_in_subdirectory() {
    let src = make_source(&[("root.txt", "r")]);
    let ws = stage(src.path());
    fs::create_dir_all(ws.path().join("sub")).unwrap();
    fs::write(ws.path().join("sub/nested.txt"), "nested").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(!summary.added.is_empty());
}

#[test]
fn diff_analyzer_new_file_change_type() {
    let src = make_source(&[("orig.txt", "o")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("added.txt"), "a").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    assert!(
        diff.files_added
            .iter()
            .any(|fc| fc.path == PathBuf::from("added.txt")),
        "new file should appear in files_added"
    );
}

#[test]
fn diff_new_file_counts_all_lines_as_additions() {
    let src = make_source(&[("base.txt", "b")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("multi.txt"), "line1\nline2\nline3\n").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_additions >= 3);
}

// ═══════════════════════════════════════════════════════════════════════════
//  5. Delete files — removed files appear in diff
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_detects_deleted_file() {
    let src = make_source(&[("remove_me.txt", "gone")]);
    let ws = stage(src.path());
    fs::remove_file(ws.path().join("remove_me.txt")).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.deleted.len(), 1);
    assert_eq!(summary.deleted[0], PathBuf::from("remove_me.txt"));
}

#[test]
fn diff_detects_multiple_deleted_files() {
    let src = make_source(&[("d1.txt", "1"), ("d2.txt", "2"), ("keep.txt", "k")]);
    let ws = stage(src.path());
    fs::remove_file(ws.path().join("d1.txt")).unwrap();
    fs::remove_file(ws.path().join("d2.txt")).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.deleted.len(), 2);
}

#[test]
fn diff_deleted_counts_all_lines_as_deletions() {
    let src = make_source(&[("multi.txt", "l1\nl2\nl3\n")]);
    let ws = stage(src.path());
    fs::remove_file(ws.path().join("multi.txt")).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_deletions >= 3);
}

#[test]
fn diff_analyzer_deleted_change_type() {
    let src = make_source(&[("del.txt", "x")]);
    let ws = stage(src.path());
    fs::remove_file(ws.path().join("del.txt")).unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    assert!(
        diff.files_deleted
            .iter()
            .any(|fc| fc.path == PathBuf::from("del.txt")),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  6. .git exclusion — .git directory not copied
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn git_dir_not_copied_from_source() {
    let src = make_source(&[("f.txt", "val")]);
    // Create a fake .git dir in the source.
    fs::create_dir_all(src.path().join(".git/objects")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The staged workspace should NOT have the source .git.
    assert!(
        !ws.path().join(".git/HEAD").exists() || {
            // If git_init was off, .git shouldn't exist at all.
            !ws.path().join(".git").exists()
        }
    );
}

#[test]
fn git_dir_excluded_from_file_listing() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = stage(src.path());
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.contains(".git")));
}

#[test]
fn source_git_objects_not_in_staged_workspace() {
    let src = make_source(&[("f.txt", "v")]);
    let src_git = src.path().join(".git/objects/pack");
    fs::create_dir_all(&src_git).unwrap();
    fs::write(src_git.join("dummy.pack"), "pack data").unwrap();

    let ws = stage(src.path());
    // The staged .git is a fresh git init, should not contain source pack files.
    let staged_pack = ws.path().join(".git/objects/pack/dummy.pack");
    assert!(
        !staged_pack.exists(),
        "source .git objects must not be copied"
    );
}

#[test]
fn snapshot_excludes_git_directory() {
    let src = make_source(&[("snap.txt", "data")]);
    let ws = stage(src.path());
    let snap = capture(ws.path()).unwrap();
    for key in snap.files.keys() {
        assert!(
            !key.to_string_lossy().contains(".git"),
            "snapshot should exclude .git: {key:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  7. Glob filtering — include/exclude patterns work
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn glob_exclude_txt_files() {
    let src = make_source(&[("keep.rs", "fn main(){}"), ("skip.txt", "text")]);
    let ws = stage_with_globs(src.path(), vec![], vec!["*.txt".into()]);
    let files = collect_files(ws.path());
    assert!(files.contains(&"keep.rs".to_string()));
    assert!(!files.contains(&"skip.txt".to_string()));
}

#[test]
fn glob_include_only_rs_files() {
    let src = make_source(&[("code.rs", "fn x(){}"), ("data.json", "{}")]);
    let ws = stage_with_globs(src.path(), vec!["*.rs".into()], vec![]);
    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(!files.contains(&"data.json".to_string()));
}

#[test]
fn glob_exclude_directory_pattern() {
    let src = make_source(&[
        ("src/main.rs", "fn main(){}"),
        ("target/debug/binary", "bin"),
    ]);
    let ws = stage_with_globs(src.path(), vec![], vec!["target/**".into()]);
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("main.rs")));
    assert!(!files.iter().any(|f| f.contains("target")));
}

#[test]
fn glob_combined_include_exclude() {
    let src = make_source(&[
        ("src/lib.rs", "pub fn f(){}"),
        ("src/lib.log", "log data"),
        ("docs/readme.md", "# Docs"),
    ]);
    let ws = stage_with_globs(src.path(), vec!["src/**".into()], vec!["*.log".into()]);
    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("lib.rs")));
    assert!(!files.iter().any(|f| f.contains("lib.log")));
    assert!(!files.iter().any(|f| f.contains("readme.md")));
}

#[test]
fn glob_exclude_takes_precedence() {
    let src = make_source(&[("a.rs", "rs"), ("a.txt", "txt")]);
    // Include everything, but exclude .txt.
    let ws = stage_with_globs(src.path(), vec!["*".into()], vec!["*.txt".into()]);
    let files = collect_files(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
    assert!(!files.contains(&"a.txt".to_string()));
}

#[test]
fn glob_via_workspace_manager() {
    let src = make_source(&[("inc.rs", "fn(){}"), ("exc.log", "log")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["*.log".into()],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".log")));
}

// ═══════════════════════════════════════════════════════════════════════════
//  8. Large workspace — many files handled efficiently
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn large_workspace_100_files() {
    let tmp = TempDir::new().unwrap();
    for i in 0..100 {
        fs::write(
            tmp.path().join(format!("file_{i:03}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    let ws = stage(tmp.path());
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 100);
}

#[test]
fn large_workspace_diff_after_bulk_modify() {
    let tmp = TempDir::new().unwrap();
    for i in 0..50 {
        fs::write(tmp.path().join(format!("f{i}.txt")), format!("v{i}")).unwrap();
    }
    let ws = stage(tmp.path());
    for i in 0..50 {
        fs::write(ws.path().join(format!("f{i}.txt")), format!("modified_{i}")).unwrap();
    }
    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 50);
}

#[test]
fn large_workspace_snapshot_captures_all() {
    let tmp = TempDir::new().unwrap();
    for i in 0..75 {
        fs::write(tmp.path().join(format!("s{i}.txt")), format!("data{i}")).unwrap();
    }
    let ws = stage(tmp.path());
    let snap = capture(ws.path()).unwrap();
    assert_eq!(snap.file_count(), 75);
}

// ═══════════════════════════════════════════════════════════════════════════
//  9. Nested directories — deep directory trees preserved
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn nested_three_levels_deep() {
    let src = make_source(&[("a/b/c/deep.txt", "deep")]);
    let ws = stage(src.path());
    assert!(ws.path().join("a/b/c/deep.txt").exists());
}

#[test]
fn nested_preserves_contents_at_depth() {
    let src = make_source(&[("x/y/z/data.txt", "nested data")]);
    let ws = stage(src.path());
    let content = fs::read_to_string(ws.path().join("x/y/z/data.txt")).unwrap();
    assert_eq!(content, "nested data");
}

#[test]
fn nested_diff_detects_change_in_deep_file() {
    let src = make_source(&[("d1/d2/d3/f.txt", "original")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("d1/d2/d3/f.txt"), "changed").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(!summary.modified.is_empty());
}

#[test]
fn nested_empty_intermediate_dirs_preserved() {
    let src = make_source(&[("a/b/leaf.txt", "leaf"), ("a/c/other.txt", "other")]);
    let ws = stage(src.path());
    assert!(ws.path().join("a/b/leaf.txt").exists());
    assert!(ws.path().join("a/c/other.txt").exists());
}

#[test]
fn nested_five_levels_deep() {
    let src = make_source(&[("l1/l2/l3/l4/l5/bottom.txt", "bottom")]);
    let ws = stage(src.path());
    let content = fs::read_to_string(ws.path().join("l1/l2/l3/l4/l5/bottom.txt")).unwrap();
    assert_eq!(content, "bottom");
}

#[test]
fn nested_new_file_in_new_subdir_in_diff() {
    let src = make_source(&[("top.txt", "t")]);
    let ws = stage(src.path());
    fs::create_dir_all(ws.path().join("new_dir/sub")).unwrap();
    fs::write(ws.path().join("new_dir/sub/new.txt"), "new").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(!summary.added.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
//  10. Symlinks — symlink handling (skip by default)
// ═══════════════════════════════════════════════════════════════════════════

// Note: copy_workspace uses `follow_links(false)` so symlinks are skipped
// (not followed). These tests verify that behavior.

#[cfg(unix)]
#[test]
fn symlink_file_not_followed_during_staging() {
    let src = make_source(&[("real.txt", "real content")]);
    std::os::unix::fs::symlink(src.path().join("real.txt"), src.path().join("link.txt")).unwrap();
    let ws = stage(src.path());
    assert!(ws.path().join("real.txt").exists());
    // Symlinks are skipped (not regular files, not dirs).
    assert!(
        !ws.path().join("link.txt").exists(),
        "symlink should be skipped"
    );
}

#[cfg(unix)]
#[test]
fn symlink_dir_not_followed_during_staging() {
    let src = make_source(&[("real_dir/file.txt", "content")]);
    std::os::unix::fs::symlink(src.path().join("real_dir"), src.path().join("link_dir")).unwrap();
    let ws = stage(src.path());
    assert!(ws.path().join("real_dir/file.txt").exists());
    assert!(
        !ws.path().join("link_dir").exists(),
        "symlink dir should be skipped"
    );
}

#[cfg(windows)]
#[test]
fn symlink_skipped_on_windows() {
    // On Windows, symlinks may require admin privileges. We simply verify
    // that staging works without error when no symlinks are present.
    let src = make_source(&[("normal.txt", "data")]);
    let ws = stage(src.path());
    assert!(ws.path().join("normal.txt").exists());
}

// ═══════════════════════════════════════════════════════════════════════════
//  11. Binary files — binary files in workspace
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn binary_file_copied_correctly() {
    let src = TempDir::new().unwrap();
    let data: Vec<u8> = (0u8..=255).collect();
    fs::write(src.path().join("binary.bin"), &data).unwrap();
    let ws = stage(src.path());
    let copied = fs::read(ws.path().join("binary.bin")).unwrap();
    assert_eq!(copied, data);
}

#[test]
fn binary_file_in_snapshot_detected_as_binary() {
    let src = TempDir::new().unwrap();
    // Create content with null bytes (binary indicator).
    let data: Vec<u8> = vec![0u8, 1, 2, 0, 3, 4, 0, 5];
    fs::write(src.path().join("bin.dat"), &data).unwrap();
    let ws = stage(src.path());
    let snap = capture(ws.path()).unwrap();
    let file_snap = snap.get_file(Path::new("bin.dat")).unwrap();
    assert!(file_snap.is_binary, "file with null bytes should be binary");
}

#[test]
fn binary_file_diff_shows_in_numstat() {
    let src = TempDir::new().unwrap();
    let data = vec![0u8; 64];
    fs::write(src.path().join("b.bin"), &data).unwrap();
    let ws = stage(src.path());
    // Modify the binary file.
    fs::write(ws.path().join("b.bin"), &[1u8; 64]).unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(
        !summary.is_empty(),
        "binary modification should be detected"
    );
}

#[test]
fn text_file_not_detected_as_binary() {
    let src = make_source(&[("text.txt", "hello world\nline 2")]);
    let ws = stage(src.path());
    let snap = capture(ws.path()).unwrap();
    let file_snap = snap.get_file(Path::new("text.txt")).unwrap();
    assert!(!file_snap.is_binary);
}

// ═══════════════════════════════════════════════════════════════════════════
//  12. Cleanup — workspace properly cleaned up after use
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_temp_dir_removed_on_drop() {
    let src = make_source(&[("drop.txt", "bye")]);
    let ws = stage(src.path());
    let ws_path = ws.path().to_path_buf();
    assert!(ws_path.exists());
    drop(ws);
    assert!(
        !ws_path.exists(),
        "temp dir should be removed after dropping PreparedWorkspace"
    );
}

#[test]
fn workspace_cleanup_removes_nested_files() {
    let src = make_source(&[("a/b/c.txt", "nested")]);
    let ws = stage(src.path());
    let ws_path = ws.path().to_path_buf();
    drop(ws);
    assert!(!ws_path.exists());
}

#[test]
fn passthrough_workspace_not_cleaned_up() {
    let src = make_source(&[("keep.txt", "keep")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let ws_path = ws.path().to_path_buf();
    drop(ws);
    assert!(
        ws_path.exists(),
        "pass-through workspace must not be deleted"
    );
}

#[test]
fn multiple_workspaces_each_cleaned_independently() {
    let src = make_source(&[("ind.txt", "i")]);
    let ws1 = stage(src.path());
    let ws2 = stage(src.path());
    let p1 = ws1.path().to_path_buf();
    let p2 = ws2.path().to_path_buf();
    drop(ws1);
    assert!(!p1.exists());
    assert!(p2.exists(), "ws2 should still exist");
    drop(ws2);
    assert!(!p2.exists());
}

// ═══════════════════════════════════════════════════════════════════════════
//  13. Concurrent workspaces — multiple workspaces simultaneously
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_workspaces_have_distinct_paths() {
    let src = make_source(&[("c.txt", "concurrent")]);
    let ws1 = stage(src.path());
    let ws2 = stage(src.path());
    assert_ne!(ws1.path(), ws2.path());
}

#[test]
fn concurrent_workspaces_independent_modifications() {
    let src = make_source(&[("shared.txt", "original")]);
    let ws1 = stage(src.path());
    let ws2 = stage(src.path());
    fs::write(ws1.path().join("shared.txt"), "ws1 change").unwrap();
    // ws2 should still have original.
    let ws2_content = fs::read_to_string(ws2.path().join("shared.txt")).unwrap();
    assert_eq!(ws2_content, "original");
}

#[test]
fn concurrent_workspaces_independent_diffs() {
    let src = make_source(&[("f.txt", "orig")]);
    let ws1 = stage(src.path());
    let ws2 = stage(src.path());
    fs::write(ws1.path().join("f.txt"), "ws1").unwrap();
    let diff1 = diff_workspace(&ws1).unwrap();
    let diff2 = diff_workspace(&ws2).unwrap();
    assert!(!diff1.is_empty());
    assert!(diff2.is_empty());
}

#[test]
fn concurrent_workspaces_five_at_once() {
    let src = make_source(&[("multi.txt", "m")]);
    let workspaces: Vec<PreparedWorkspace> = (0..5).map(|_| stage(src.path())).collect();
    let paths: Vec<PathBuf> = workspaces.iter().map(|w| w.path().to_path_buf()).collect();
    // All paths are distinct.
    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            assert_ne!(paths[i], paths[j]);
        }
    }
}

#[test]
fn concurrent_git_operations_independent() {
    let src = make_source(&[("g.txt", "git")]);
    let ws1 = stage(src.path());
    let ws2 = stage(src.path());

    // Modify only ws1.
    fs::write(ws1.path().join("g.txt"), "changed").unwrap();

    let status1 = WorkspaceManager::git_status(ws1.path()).unwrap_or_default();
    let status2 = WorkspaceManager::git_status(ws2.path()).unwrap_or_default();

    assert!(!status1.is_empty(), "ws1 should have changes");
    assert!(status2.is_empty(), "ws2 should be clean");
}

// ═══════════════════════════════════════════════════════════════════════════
//  14. Path normalization — paths normalized across platforms
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn path_normalization_forward_slashes_in_diff() {
    let src = make_source(&[("sub/dir/file.txt", "data")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("sub/dir/file.txt"), "changed").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    // Git always uses forward slashes in its output.
    for path in &summary.modified {
        let s = path.to_string_lossy();
        assert!(
            !s.contains('\\'),
            "diff paths should use forward slashes: {s}"
        );
    }
}

#[test]
fn path_normalization_snapshot_relative_paths() {
    let src = make_source(&[("a/b.txt", "content")]);
    let ws = stage(src.path());
    let snap = capture(ws.path()).unwrap();
    for key in snap.files.keys() {
        assert!(
            !key.starts_with("/") && !key.starts_with("\\"),
            "snapshot paths should be relative: {key:?}"
        );
    }
}

#[test]
fn path_normalization_collect_files_uses_forward_slashes() {
    let src = make_source(&[("nested/path/file.txt", "v")]);
    let ws = stage(src.path());
    let files = collect_files(ws.path());
    for f in &files {
        assert!(
            !f.contains('\\'),
            "collected paths should use forward slashes: {f}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Extra coverage: Snapshot comparison, templates, tracker, ops
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_compare_detects_added_file() {
    let src = make_source(&[("base.txt", "b")]);
    let ws = stage(src.path());
    let snap_before = capture(ws.path()).unwrap();
    fs::write(ws.path().join("new.txt"), "n").unwrap();
    let snap_after = capture(ws.path()).unwrap();
    let diff = compare(&snap_before, &snap_after);
    assert!(diff.added.contains(&PathBuf::from("new.txt")));
}

#[test]
fn snapshot_compare_detects_removed_file() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = stage(src.path());
    let snap_before = capture(ws.path()).unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();
    let snap_after = capture(ws.path()).unwrap();
    let diff = compare(&snap_before, &snap_after);
    assert!(diff.removed.contains(&PathBuf::from("b.txt")));
}

#[test]
fn snapshot_compare_detects_modified_file() {
    let src = make_source(&[("m.txt", "before")]);
    let ws = stage(src.path());
    let snap_before = capture(ws.path()).unwrap();
    fs::write(ws.path().join("m.txt"), "after").unwrap();
    let snap_after = capture(ws.path()).unwrap();
    let diff = compare(&snap_before, &snap_after);
    assert!(diff.modified.contains(&PathBuf::from("m.txt")));
}

#[test]
fn snapshot_compare_unchanged_files() {
    let src = make_source(&[("same.txt", "unchanged")]);
    let ws = stage(src.path());
    let snap1 = capture(ws.path()).unwrap();
    let snap2 = capture(ws.path()).unwrap();
    let diff = compare(&snap1, &snap2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert!(!diff.unchanged.is_empty());
}

#[test]
fn snapshot_total_size() {
    let src = make_source(&[("s.txt", "12345")]);
    let ws = stage(src.path());
    let snap = capture(ws.path()).unwrap();
    assert_eq!(snap.total_size(), 5);
}

#[test]
fn snapshot_has_file_check() {
    let src = make_source(&[("exists.txt", "y")]);
    let ws = stage(src.path());
    let snap = capture(ws.path()).unwrap();
    assert!(snap.has_file(Path::new("exists.txt")));
    assert!(!snap.has_file(Path::new("nope.txt")));
}

#[test]
fn change_tracker_records_and_summarizes() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "new.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "old.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(50),
        size_after: None,
        content_hash: None,
    });
    let summary = tracker.summary();
    assert_eq!(summary.created, 1);
    assert_eq!(summary.deleted, 1);
    assert!(tracker.has_changes());
}

#[test]
fn change_tracker_affected_paths() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(10),
        size_after: Some(20),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(5),
        content_hash: None,
    });
    let paths = tracker.affected_paths();
    assert_eq!(paths, vec!["a.txt", "b.txt"]);
}

#[test]
fn change_tracker_clear() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(1),
        content_hash: None,
    });
    tracker.clear();
    assert!(!tracker.has_changes());
}

#[test]
fn operation_log_records_and_queries() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Write {
        path: "out.txt".into(),
        size: 42,
    });
    log.record(FileOperation::Read {
        path: "in.txt".into(),
    });
    log.record(FileOperation::Delete {
        path: "old.txt".into(),
    });
    assert_eq!(log.writes(), vec!["out.txt"]);
    assert_eq!(log.reads(), vec!["in.txt"]);
    assert_eq!(log.deletes(), vec!["old.txt"]);
    let summary = log.summary();
    assert_eq!(summary.writes, 1);
    assert_eq!(summary.reads, 1);
    assert_eq!(summary.deletes, 1);
    assert_eq!(summary.total_writes_bytes, 42);
}

#[test]
fn operation_filter_allows_and_denies() {
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.log");
    assert!(!filter.is_allowed("server.log"));
    assert!(filter.is_allowed("main.rs"));
}

#[test]
fn operation_filter_filters_operations() {
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.tmp");
    let ops = vec![
        FileOperation::Write {
            path: "good.txt".into(),
            size: 10,
        },
        FileOperation::Write {
            path: "bad.tmp".into(),
            size: 5,
        },
    ];
    let allowed = filter.filter_operations(&ops);
    assert_eq!(allowed.len(), 1);
}

#[test]
fn diff_workspace_summary_string() {
    let diff = WorkspaceDiff::default();
    assert_eq!(diff.summary(), "No changes detected.");
}

#[test]
fn diff_workspace_summary_with_changes() {
    let src = make_source(&[("s.txt", "v1")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("s.txt"), "v2").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    let summary_str = diff.summary();
    assert!(summary_str.contains("changed"));
}

#[test]
fn diff_policy_pass_when_within_limits() {
    let diff = WorkspaceDiff::default();
    let policy = DiffPolicy {
        max_files: Some(10),
        max_additions: Some(100),
        denied_paths: vec![],
    };
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_fail_max_files_exceeded() {
    let src = make_source(&[("a.txt", "a")]);
    let ws = stage(src.path());
    for i in 0..5 {
        fs::write(ws.path().join(format!("new{i}.txt")), "data").unwrap();
    }
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    let policy = DiffPolicy {
        max_files: Some(2),
        max_additions: None,
        denied_paths: vec![],
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

#[test]
fn diff_analysis_parse_empty() {
    let analysis = DiffAnalysis::parse("");
    assert!(analysis.is_empty());
    assert_eq!(analysis.file_count(), 0);
}

#[test]
fn diff_analysis_parse_simple_add() {
    let raw = "\
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+hello
+world
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.total_additions, 2);
}

#[test]
fn diff_analysis_parse_modification() {
    let raw = "\
diff --git a/f.txt b/f.txt
--- a/f.txt
+++ b/f.txt
@@ -1 +1 @@
-old
+new
";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.total_additions, 1);
    assert_eq!(analysis.total_deletions, 1);
}

#[test]
fn workspace_stager_missing_source_root_errors() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
}

#[test]
fn workspace_stager_nonexistent_source_errors() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
}

#[test]
fn workspace_manager_git_status_on_clean_workspace() {
    let src = make_source(&[("s.txt", "clean")]);
    let ws = stage(src.path());
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(status.unwrap().trim().is_empty());
}

#[test]
fn workspace_manager_git_diff_on_clean_workspace() {
    let src = make_source(&[("d.txt", "clean")]);
    let ws = stage(src.path());
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    assert!(diff.unwrap().trim().is_empty());
}

#[test]
fn workspace_manager_git_diff_after_modification() {
    let src = make_source(&[("d.txt", "before\n")]);
    let ws = stage(src.path());
    fs::write(ws.path().join("d.txt"), "after\n").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    let diff_text = diff.unwrap();
    assert!(diff_text.contains("-before"));
    assert!(diff_text.contains("+after"));
}

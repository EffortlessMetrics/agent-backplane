// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive workspace staging tests (80+).
//!
//! Categories:
//! 1. Workspace creation from source directory
//! 2. Glob-based file inclusion/exclusion
//! 3. .git directory auto-exclusion
//! 4. Git initialization in staged workspace
//! 5. Baseline commit creation
//! 6. Diff generation after modifications
//! 7. Cleanup on drop
//! 8. Edge cases (empty directories, deeply nested, symlinks)
//! 9. Error handling (non-existent source, missing source_root)
//! 10. Multiple workspaces in parallel
//! 11. WorkspaceStager builder API
//! 12. Snapshot capture/compare
//! 13. DiffSummary analysis
//! 14. Template system
//! 15. ChangeTracker / OperationLog

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{DiffSummary, diff_workspace};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog, OperationSummary};
use abp_workspace::snapshot::{capture, compare};
use abp_workspace::template::{TemplateRegistry, WorkspaceTemplate};
use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn staged_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

fn staged_spec_globs(root: &Path, include: Vec<String>, exclude: Vec<String>) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
    }
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

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn create_fixture(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src").join("data.json"), "{}").unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_one.rs"),
        "#[test] fn t() {}",
    )
    .unwrap();
}

/// Helper: create source dir with given files and return TempDir.
fn make_source(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = tempdir().unwrap();
    for (rel, content) in files {
        let p = tmp.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
    }
    tmp
}

// ===========================================================================
// 1. Workspace creation from source directory
// ===========================================================================

#[test]
fn creation_copies_all_files_from_flat_source() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b"), ("c.txt", "c")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()), vec!["a.txt", "b.txt", "c.txt"]);
}

#[test]
fn creation_copies_nested_files() {
    let src = make_source(&[
        ("root.txt", "r"),
        ("d1/f1.txt", "1"),
        ("d1/d2/f2.txt", "2"),
    ]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        collect_files(ws.path()),
        vec!["d1/d2/f2.txt", "d1/f1.txt", "root.txt"]
    );
}

#[test]
fn creation_preserves_file_contents() {
    let content = "hello 世界 🚀";
    let src = make_source(&[("uni.txt", content)]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("uni.txt")).unwrap(), content);
}

#[test]
fn creation_preserves_binary_content() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("bin.dat"), &data).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("bin.dat")).unwrap(), data);
}

#[test]
fn creation_staged_path_differs_from_source() {
    let src = make_source(&[("f.txt", "x")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn creation_does_not_modify_source() {
    let src = make_source(&[("f.txt", "original")]);
    let before = collect_files(src.path());
    let _ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(src.path()), before);
    assert_eq!(
        fs::read_to_string(src.path().join("f.txt")).unwrap(),
        "original"
    );
}

#[test]
fn creation_handles_file_with_spaces_in_name() {
    let src = make_source(&[("hello world.txt", "hi")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("hello world.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("hello world.txt")).unwrap(),
        "hi"
    );
}

#[test]
fn creation_handles_dotfiles() {
    let src = make_source(&[(".hidden", "secret"), (".config/app.toml", "[app]")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".hidden").exists());
    assert!(ws.path().join(".config").join("app.toml").exists());
}

// ===========================================================================
// 2. Glob-based file inclusion/exclusion
// ===========================================================================

#[test]
fn glob_include_only_rs_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec![],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected non-.rs file: {f}");
    }
    assert!(!files.is_empty());
}

#[test]
fn glob_exclude_json_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["*.json".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".json")));
}

#[test]
fn glob_exclude_directory() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["tests/**".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
}

#[test]
fn glob_include_and_exclude_combined() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec!["tests/**".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(files.iter().all(|f| f.ends_with(".rs")));
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
}

#[test]
fn glob_multiple_include_patterns() {
    let src = make_source(&[
        ("code.rs", "fn f() {}"),
        ("cfg.toml", "[p]"),
        ("notes.md", "# N"),
    ]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.rs".into(), "*.toml".into()],
        vec![],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(files.contains(&"cfg.toml".to_string()));
    assert!(!files.contains(&"notes.md".to_string()));
}

#[test]
fn glob_multiple_exclude_patterns() {
    let src = make_source(&[
        ("keep.rs", "x"),
        ("drop.log", "y"),
        ("drop.tmp", "z"),
    ]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["*.log".into(), "*.tmp".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files, vec!["keep.rs"]);
}

#[test]
fn glob_exclude_overrides_include() {
    let src = make_source(&[
        ("src/lib.rs", "pub fn a() {}"),
        ("src/generated/out.rs", "// gen"),
    ]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/generated/**".into()],
    ))
    .unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.iter().any(|f| f.starts_with("src/generated/")));
}

#[test]
fn glob_empty_patterns_copies_everything() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec![])).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

// ===========================================================================
// 3. .git directory auto-exclusion
// ===========================================================================

#[test]
fn dot_git_never_copied_with_default_stager() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git/objects")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn dot_git_never_copied_via_workspace_manager() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "x").unwrap();
    fs::write(src.path().join("data.txt"), "d").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
}

#[test]
fn dot_git_not_copied_even_with_include_double_star() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "y").unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("a.txt").exists());
}

// ===========================================================================
// 4. Git initialization in staged workspace
// ===========================================================================

#[test]
fn git_init_creates_dot_git_dir() {
    let src = make_source(&[("f.txt", "d")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_git_init_enabled_by_default() {
    let src = make_source(&[("f.txt", "d")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_git_init_disabled() {
    let src = make_source(&[("f.txt", "d")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn git_init_working_tree_is_clean() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty(), "expected clean tree, got: {status}");
}

// ===========================================================================
// 5. Baseline commit creation
// ===========================================================================

#[test]
fn baseline_commit_exists() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--format=%s"]);
    assert!(log.contains("baseline"));
}

#[test]
fn exactly_one_commit_after_staging() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1");
}

#[test]
fn baseline_commit_includes_all_files() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(files.contains("a.txt"));
    assert!(files.contains("b.txt"));
}

#[test]
fn baseline_commit_author_is_abp() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let author = git(ws.path(), &["log", "--format=%an"]);
    assert_eq!(author, "abp");
}

// ===========================================================================
// 6. Diff generation after modifications
// ===========================================================================

#[test]
fn modified_file_in_git_diff() {
    let src = make_source(&[("data.txt", "original")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "modified").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path()).expect("diff");
    assert!(diff.contains("data.txt"));
    assert!(diff.contains("modified"));
}

#[test]
fn new_file_in_git_status() {
    let src = make_source(&[("existing.txt", "hi")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new.txt"), "new").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).expect("status");
    assert!(status.contains("new.txt"));
}

#[test]
fn deleted_file_in_git_status() {
    let src = make_source(&[("doomed.txt", "bye")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let status = WorkspaceManager::git_status(ws.path()).expect("status");
    assert!(status.contains("doomed.txt"));
    assert!(status.contains(" D "));
}

#[test]
fn diff_summary_detects_added_file() {
    let src = make_source(&[("existing.txt", "hi")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new_file.txt"), "new content\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary.added.iter().any(|p| p.to_string_lossy().contains("new_file.txt")),
        "expected new_file.txt in added: {summary:?}"
    );
}

#[test]
fn diff_summary_detects_modified_file() {
    let src = make_source(&[("mod.txt", "before\n")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("mod.txt"), "after\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary.modified.iter().any(|p| p.to_string_lossy().contains("mod.txt")),
        "expected mod.txt in modified: {summary:?}"
    );
}

#[test]
fn diff_summary_detects_deleted_file() {
    let src = make_source(&[("del.txt", "gone\n")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("del.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary.deleted.iter().any(|p| p.to_string_lossy().contains("del.txt")),
        "expected del.txt in deleted: {summary:?}"
    );
}

#[test]
fn diff_summary_counts_line_changes() {
    let src = make_source(&[("lines.txt", "line1\nline2\nline3\n")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("lines.txt"), "line1\nchanged\nline3\nnew\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_additions > 0);
    assert!(summary.total_deletions > 0);
}

#[test]
fn diff_summary_empty_when_no_changes() {
    let src = make_source(&[("stable.txt", "same\n")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

#[test]
fn diff_summary_file_count_aggregates() {
    let src = make_source(&[("a.txt", "a\n"), ("b.txt", "b\n"), ("c.txt", "c\n")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("a.txt"), "aa\n").unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();
    fs::write(ws.path().join("d.txt"), "new\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.file_count(), 3);
}

// ===========================================================================
// 7. Cleanup on drop
// ===========================================================================

#[test]
fn manager_workspace_cleaned_on_drop() {
    let src = make_source(&[("f.txt", "data")]);
    let staged_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists());
    }
    assert!(!staged_path.exists());
}

#[test]
fn stager_workspace_cleaned_on_drop() {
    let src = make_source(&[("f.txt", "data")]);
    let staged_path;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists());
    }
    assert!(!staged_path.exists());
}

#[test]
fn cleanup_removes_deeply_nested_content() {
    let src = tempdir().unwrap();
    let mut deep = src.path().to_path_buf();
    for i in 0..8 {
        deep = deep.join(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "deep").unwrap();

    let staged_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();
    }
    assert!(!staged_path.exists());
}

// ===========================================================================
// 8. Edge cases
// ===========================================================================

#[test]
fn empty_source_directory_stages_ok() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn empty_subdirectories_are_created() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_dir")).unwrap();
    fs::write(src.path().join("root.txt"), "hi").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("empty_dir").exists());
    assert!(ws.path().join("root.txt").exists());
}

#[test]
fn deeply_nested_directory_10_levels() {
    let src = tempdir().unwrap();
    let mut deep = src.path().to_path_buf();
    for i in 0..10 {
        deep = deep.join(format!("level{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "bottom").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let mut expected = ws.path().to_path_buf();
    for i in 0..10 {
        expected = expected.join(format!("level{i}"));
    }
    assert_eq!(
        fs::read_to_string(expected.join("leaf.txt")).unwrap(),
        "bottom"
    );
}

#[test]
fn large_number_of_files() {
    let src = tempdir().unwrap();
    for i in 0..100 {
        fs::write(
            src.path().join(format!("file_{i:03}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()).len(), 100);
}

#[test]
fn large_single_file_content_preserved() {
    let src = tempdir().unwrap();
    let big = "X".repeat(512 * 1024);
    fs::write(src.path().join("big.bin"), &big).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("big.bin")).unwrap().len(),
        big.len()
    );
}

#[test]
fn symlinks_handled_without_error() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src.path().join("real.txt"), src.path().join("link.txt"))
            .unwrap();
    }
    #[cfg(windows)]
    {
        let _ = std::os::windows::fs::symlink_file(
            src.path().join("real.txt"),
            src.path().join("link.txt"),
        );
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("real.txt").exists());
}

#[test]
fn unicode_filename_preserved() {
    let src = make_source(&[("données/résumé.txt", "contenu")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("données").join("résumé.txt")).unwrap(),
        "contenu"
    );
}

#[test]
fn parallel_sibling_directories() {
    let src = tempdir().unwrap();
    for d in &["alpha", "beta", "gamma"] {
        fs::create_dir_all(src.path().join(d)).unwrap();
        fs::write(src.path().join(d).join("file.txt"), d).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    for d in &["alpha", "beta", "gamma"] {
        assert_eq!(
            fs::read_to_string(ws.path().join(d).join("file.txt")).unwrap(),
            *d
        );
    }
}

// ===========================================================================
// 9. Error handling
// ===========================================================================

#[test]
fn error_nonexistent_source_directory() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/xyz_does_not_exist")
        .stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("does not exist"), "error: {msg}");
}

#[test]
fn error_no_source_root_set() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("source_root"), "error: {msg}");
}

#[test]
fn error_invalid_include_glob() {
    let src = make_source(&[("f.txt", "x")]);
    let result = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["[".into()],
        vec![],
    ));
    assert!(result.is_err());
}

#[test]
fn error_invalid_exclude_glob() {
    let src = make_source(&[("f.txt", "x")]);
    let result = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["[".into()],
    ));
    assert!(result.is_err());
}

// ===========================================================================
// 10. Multiple workspaces in parallel
// ===========================================================================

#[test]
fn multiple_independent_staged_workspaces() {
    let src = make_source(&[("shared.txt", "original")]);
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_ne!(ws1.path(), ws2.path());

    fs::write(ws1.path().join("shared.txt"), "ws1").unwrap();
    fs::write(ws2.path().join("shared.txt"), "ws2").unwrap();

    assert_eq!(
        fs::read_to_string(ws1.path().join("shared.txt")).unwrap(),
        "ws1"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "ws2"
    );
    assert_eq!(
        fs::read_to_string(src.path().join("shared.txt")).unwrap(),
        "original"
    );
}

#[test]
fn three_workspaces_from_same_source() {
    let src = make_source(&[("data.txt", "d")]);
    let w1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let w2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let w3 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let paths: Vec<PathBuf> = vec![
        w1.path().to_path_buf(),
        w2.path().to_path_buf(),
        w3.path().to_path_buf(),
    ];
    // All different paths
    assert_ne!(paths[0], paths[1]);
    assert_ne!(paths[1], paths[2]);
    assert_ne!(paths[0], paths[2]);
}

#[test]
fn workspaces_with_different_globs_from_same_source() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws_rs = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec![],
    ))
    .unwrap();
    let ws_md = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.md".into()],
        vec![],
    ))
    .unwrap();

    let rs_files = collect_files(ws_rs.path());
    let md_files = collect_files(ws_md.path());

    assert!(rs_files.iter().all(|f| f.ends_with(".rs")));
    assert!(md_files.iter().all(|f| f.ends_with(".md")));
}

// ===========================================================================
// 11. WorkspaceStager builder API
// ===========================================================================

#[test]
fn stager_default_creates_new() {
    let s = WorkspaceStager::default();
    // Default is same as new() — no source_root set, so stage should fail.
    assert!(s.stage().is_err());
}

#[test]
fn stager_builder_source_and_stage() {
    let src = make_source(&[("hello.txt", "world")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("hello.txt").exists());
}

#[test]
fn stager_builder_with_include() {
    let src = make_source(&[("a.rs", "fn a() {}"), ("b.md", "# B")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
    assert!(!files.contains(&"b.md".to_string()));
}

#[test]
fn stager_builder_with_exclude() {
    let src = make_source(&[("keep.rs", "fn k() {}"), ("drop.log", "log")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"keep.rs".to_string()));
    assert!(!files.contains(&"drop.log".to_string()));
}

#[test]
fn stager_builder_chaining() {
    let src = make_source(&[("src/lib.rs", "pub fn x() {}"), ("target/out.o", "bin")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["*.o".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.iter().any(|f| f.ends_with(".o")));
}

#[test]
fn stager_restage_from_staged_workspace() {
    let src = make_source(&[("original.txt", "v1")]);
    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws1.path().join("original.txt"), "v2").unwrap();

    let ws2 = WorkspaceStager::new()
        .source_root(ws1.path())
        .stage()
        .unwrap();

    assert_ne!(ws1.path(), ws2.path());
    assert_eq!(
        fs::read_to_string(ws2.path().join("original.txt")).unwrap(),
        "v2"
    );
}

// ===========================================================================
// 12. Snapshot capture and compare
// ===========================================================================

#[test]
fn snapshot_captures_all_files() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let snap = capture(ws.path()).unwrap();
    assert_eq!(snap.file_count(), 2);
}

#[test]
fn snapshot_records_file_size() {
    let content = "hello world";
    let src = make_source(&[("f.txt", content)]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let snap = capture(ws.path()).unwrap();
    assert_eq!(snap.total_size(), content.len() as u64);
}

#[test]
fn snapshot_has_file_lookup() {
    let src = make_source(&[("present.txt", "yes")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let snap = capture(ws.path()).unwrap();
    assert!(snap.has_file("present.txt"));
}

#[test]
fn snapshot_get_file_returns_hash() {
    let src = make_source(&[("hashed.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let snap = capture(ws.path()).unwrap();
    let fs = snap.get_file("hashed.txt").expect("file should exist");
    assert!(!fs.sha256.is_empty());
    assert_eq!(fs.size, 7); // "content" is 7 bytes
}

#[test]
fn snapshot_compare_identical() {
    let src = make_source(&[("f.txt", "same")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let s1 = capture(ws.path()).unwrap();
    let s2 = capture(ws.path()).unwrap();
    let diff = compare(&s1, &s2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 1);
}

#[test]
fn snapshot_compare_detects_added_file() {
    let src = make_source(&[("f.txt", "x")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let before = capture(ws.path()).unwrap();
    fs::write(ws.path().join("new.txt"), "new").unwrap();
    let after = capture(ws.path()).unwrap();

    let diff = compare(&before, &after);
    assert_eq!(diff.added.len(), 1);
}

#[test]
fn snapshot_compare_detects_removed_file() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let before = capture(ws.path()).unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();
    let after = capture(ws.path()).unwrap();

    let diff = compare(&before, &after);
    assert_eq!(diff.removed.len(), 1);
}

#[test]
fn snapshot_compare_detects_modified_file() {
    let src = make_source(&[("m.txt", "old")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let before = capture(ws.path()).unwrap();
    fs::write(ws.path().join("m.txt"), "new").unwrap();
    let after = capture(ws.path()).unwrap();

    let diff = compare(&before, &after);
    assert_eq!(diff.modified.len(), 1);
}

// ===========================================================================
// 13. DiffSummary struct methods
// ===========================================================================

#[test]
fn diff_summary_is_empty_on_default() {
    let ds = DiffSummary::default();
    assert!(ds.is_empty());
    assert_eq!(ds.file_count(), 0);
    assert_eq!(ds.total_changes(), 0);
}

#[test]
fn diff_summary_not_empty_with_added() {
    let ds = DiffSummary {
        added: vec![PathBuf::from("new.txt")],
        ..Default::default()
    };
    assert!(!ds.is_empty());
    assert_eq!(ds.file_count(), 1);
}

#[test]
fn diff_summary_total_changes_sums_add_del() {
    let ds = DiffSummary {
        total_additions: 10,
        total_deletions: 5,
        ..Default::default()
    };
    assert_eq!(ds.total_changes(), 15);
}

// ===========================================================================
// 14. Template system
// ===========================================================================

#[test]
fn template_new_is_empty() {
    let t = WorkspaceTemplate::new("test", "A test template");
    assert_eq!(t.file_count(), 0);
    assert_eq!(t.name, "test");
}

#[test]
fn template_add_and_has_file() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("src/lib.rs", "pub fn x() {}");
    assert!(t.has_file("src/lib.rs"));
    assert!(!t.has_file("missing.rs"));
    assert_eq!(t.file_count(), 1);
}

#[test]
fn template_apply_creates_files() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("src/lib.rs", "pub fn x() {}");
    t.add_file("README.md", "# Hello");

    let target = tempdir().unwrap();
    let count = t.apply(target.path()).unwrap();
    assert_eq!(count, 2);
    assert!(target.path().join("src").join("lib.rs").exists());
    assert!(target.path().join("README.md").exists());
}

#[test]
fn template_apply_creates_parent_dirs() {
    let mut t = WorkspaceTemplate::new("t", "d");
    t.add_file("a/b/c/deep.txt", "deep");

    let target = tempdir().unwrap();
    t.apply(target.path()).unwrap();
    assert_eq!(
        fs::read_to_string(target.path().join("a/b/c/deep.txt")).unwrap(),
        "deep"
    );
}

#[test]
fn template_validate_ok() {
    let t = WorkspaceTemplate::new("valid", "A valid template");
    assert!(t.validate().is_empty());
}

#[test]
fn template_validate_empty_name() {
    let t = WorkspaceTemplate::new("", "desc");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("name")));
}

#[test]
fn template_validate_empty_description() {
    let t = WorkspaceTemplate::new("name", "");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("description")));
}

#[test]
fn template_validate_absolute_path() {
    let mut t = WorkspaceTemplate::new("t", "d");
    #[cfg(windows)]
    t.add_file("C:\\bad\\path.txt", "x");
    #[cfg(unix)]
    t.add_file("/bad/path.txt", "x");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("absolute")));
}

#[test]
fn template_registry_register_and_get() {
    let mut reg = TemplateRegistry::new();
    let t = WorkspaceTemplate::new("my-tmpl", "desc");
    reg.register(t);
    assert_eq!(reg.count(), 1);
    assert!(reg.get("my-tmpl").is_some());
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn template_registry_list() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("beta", "b"));
    reg.register(WorkspaceTemplate::new("alpha", "a"));
    let names = reg.list();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn template_registry_overwrite() {
    let mut reg = TemplateRegistry::new();
    let mut t1 = WorkspaceTemplate::new("t", "old");
    t1.add_file("old.txt", "old");
    reg.register(t1);

    let mut t2 = WorkspaceTemplate::new("t", "new");
    t2.add_file("new.txt", "new");
    reg.register(t2);

    assert_eq!(reg.count(), 1);
    let t = reg.get("t").unwrap();
    assert!(t.has_file("new.txt"));
    assert!(!t.has_file("old.txt"));
}

// ===========================================================================
// 15. ChangeTracker / OperationLog
// ===========================================================================

#[test]
fn change_tracker_empty_initially() {
    let ct = ChangeTracker::new();
    assert!(!ct.has_changes());
    assert!(ct.changes().is_empty());
}

#[test]
fn change_tracker_records_created() {
    let mut ct = ChangeTracker::new();
    ct.record(FileChange {
        path: "new.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    assert!(ct.has_changes());
    let s = ct.summary();
    assert_eq!(s.created, 1);
    assert_eq!(s.total_size_delta, 100);
}

#[test]
fn change_tracker_records_multiple_kinds() {
    let mut ct = ChangeTracker::new();
    ct.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(50),
        content_hash: None,
    });
    ct.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(30),
        size_after: Some(40),
        content_hash: None,
    });
    ct.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(20),
        size_after: None,
        content_hash: None,
    });
    ct.record(FileChange {
        path: "d.txt".into(),
        kind: ChangeKind::Renamed {
            from: "old.txt".into(),
        },
        size_before: Some(10),
        size_after: Some(10),
        content_hash: None,
    });
    let s = ct.summary();
    assert_eq!(s.created, 1);
    assert_eq!(s.modified, 1);
    assert_eq!(s.deleted, 1);
    assert_eq!(s.renamed, 1);
}

#[test]
fn change_tracker_affected_paths() {
    let mut ct = ChangeTracker::new();
    ct.record(FileChange {
        path: "x.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(1),
        content_hash: None,
    });
    ct.record(FileChange {
        path: "y.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(1),
        size_after: None,
        content_hash: None,
    });
    let paths = ct.affected_paths();
    assert_eq!(paths, vec!["x.txt", "y.txt"]);
}

#[test]
fn change_tracker_by_kind() {
    let mut ct = ChangeTracker::new();
    ct.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(1),
        content_hash: None,
    });
    ct.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(2),
        content_hash: None,
    });
    ct.record(FileChange {
        path: "c.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(1),
        size_after: Some(3),
        content_hash: None,
    });
    assert_eq!(ct.by_kind(&ChangeKind::Created).len(), 2);
    assert_eq!(ct.by_kind(&ChangeKind::Modified).len(), 1);
    assert_eq!(ct.by_kind(&ChangeKind::Deleted).len(), 0);
}

#[test]
fn change_tracker_clear() {
    let mut ct = ChangeTracker::new();
    ct.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(1),
        content_hash: None,
    });
    assert!(ct.has_changes());
    ct.clear();
    assert!(!ct.has_changes());
}

#[test]
fn operation_log_empty_initially() {
    let log = OperationLog::new();
    assert!(log.operations().is_empty());
    assert!(log.reads().is_empty());
    assert!(log.writes().is_empty());
    assert!(log.deletes().is_empty());
}

#[test]
fn operation_log_records_read_write_delete() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "r.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "w.txt".into(),
        size: 42,
    });
    log.record(FileOperation::Delete {
        path: "d.txt".into(),
    });
    assert_eq!(log.reads(), vec!["r.txt"]);
    assert_eq!(log.writes(), vec!["w.txt"]);
    assert_eq!(log.deletes(), vec!["d.txt"]);
}

#[test]
fn operation_log_summary() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "r.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "w1.txt".into(),
        size: 100,
    });
    log.record(FileOperation::Write {
        path: "w2.txt".into(),
        size: 200,
    });
    log.record(FileOperation::Delete {
        path: "d.txt".into(),
    });
    log.record(FileOperation::Move {
        from: "old.txt".into(),
        to: "new.txt".into(),
    });
    log.record(FileOperation::Copy {
        from: "src.txt".into(),
        to: "dst.txt".into(),
    });
    log.record(FileOperation::CreateDir {
        path: "newdir".into(),
    });

    let s = log.summary();
    assert_eq!(s.reads, 1);
    assert_eq!(s.writes, 2);
    assert_eq!(s.deletes, 1);
    assert_eq!(s.moves, 1);
    assert_eq!(s.copies, 1);
    assert_eq!(s.create_dirs, 1);
    assert_eq!(s.total_writes_bytes, 300);
}

#[test]
fn operation_log_affected_paths() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Move {
        from: "b.txt".into(),
        to: "c.txt".into(),
    });
    let paths = log.affected_paths();
    assert!(paths.contains("a.txt"));
    assert!(paths.contains("b.txt"));
    assert!(paths.contains("c.txt"));
}

#[test]
fn operation_log_clear() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "x.txt".into(),
    });
    assert!(!log.operations().is_empty());
    log.clear();
    assert!(log.operations().is_empty());
}

#[test]
fn operation_filter_allows_all_by_default() {
    let f = OperationFilter::new();
    assert!(f.is_allowed("any/path.txt"));
}

#[test]
fn operation_filter_deny_takes_precedence() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("**/*.rs");
    f.add_denied_path("src/secret/**");
    assert!(f.is_allowed("src/lib.rs"));
    assert!(!f.is_allowed("src/secret/key.pem"));
}

#[test]
fn operation_filter_filters_operations() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.log");
    let ops = vec![
        FileOperation::Read {
            path: "app.rs".into(),
        },
        FileOperation::Read {
            path: "debug.log".into(),
        },
    ];
    let allowed = f.filter_operations(&ops);
    assert_eq!(allowed.len(), 1);
}

// ===========================================================================
// 16. PassThrough mode
// ===========================================================================

#[test]
fn passthrough_returns_original_path() {
    let src = make_source(&[("a.txt", "a")]);
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
fn passthrough_does_not_create_temp_dir() {
    let src = make_source(&[("a.txt", "a")]);
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    // The path should be exactly the source, no temp dir involved.
    assert_eq!(ws.path(), src.path());
}

// ===========================================================================
// 17. FileOperation paths helper
// ===========================================================================

#[test]
fn file_operation_read_paths() {
    let op = FileOperation::Read {
        path: "f.txt".into(),
    };
    assert_eq!(op.paths(), vec!["f.txt"]);
}

#[test]
fn file_operation_move_has_two_paths() {
    let op = FileOperation::Move {
        from: "a.txt".into(),
        to: "b.txt".into(),
    };
    assert_eq!(op.paths(), vec!["a.txt", "b.txt"]);
}

#[test]
fn file_operation_copy_has_two_paths() {
    let op = FileOperation::Copy {
        from: "s.txt".into(),
        to: "d.txt".into(),
    };
    assert_eq!(op.paths(), vec!["s.txt", "d.txt"]);
}

// ===========================================================================
// 18. OperationSummary default
// ===========================================================================

#[test]
fn operation_summary_default_all_zero() {
    let s = OperationSummary::default();
    assert_eq!(s.reads, 0);
    assert_eq!(s.writes, 0);
    assert_eq!(s.deletes, 0);
    assert_eq!(s.moves, 0);
    assert_eq!(s.copies, 0);
    assert_eq!(s.create_dirs, 0);
    assert_eq!(s.total_writes_bytes, 0);
}

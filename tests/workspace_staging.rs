// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive workspace staging integration tests.
//!
//! Covers basic staging, glob filtering, git initialisation, diff capture,
//! edge cases (deep nesting, empty dirs, large trees, cleanup-on-drop,
//! error handling, re-staging), and the `WorkspaceStager` builder.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::Path;
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

/// Run a git command in `dir` and return trimmed stdout.
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

/// Create a standard fixture tree.
fn create_fixture(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src").join("data.json"), "{}").unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests").join("test_one.rs"), "#[test] fn t() {}").unwrap();
}

// ===========================================================================
// 1. Basic staging
// ===========================================================================

#[test]
fn basic_staging_copies_all_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_eq!(
        collect_files(ws.path()),
        collect_files(src.path()),
        "staged workspace must mirror source"
    );
}

#[test]
fn basic_staging_path_differs_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_ne!(ws.path(), src.path());
}

#[test]
fn basic_staging_preserves_file_content() {
    let src = tempdir().unwrap();
    let body = "fn main() { println!(\"hello world\"); }";
    fs::write(src.path().join("main.rs"), body).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_eq!(fs::read_to_string(ws.path().join("main.rs")).unwrap(), body);
}

#[test]
fn basic_staging_does_not_modify_source() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let before = collect_files(src.path());
    let content = fs::read_to_string(src.path().join("main.rs")).unwrap();

    let _ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_eq!(collect_files(src.path()), before);
    assert_eq!(
        fs::read_to_string(src.path().join("main.rs")).unwrap(),
        content
    );
}

// ===========================================================================
// 2. Include / exclude globs
// ===========================================================================

#[test]
fn glob_include_only_rs() {
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
    assert!(
        files.iter().any(|f| f.contains('/')),
        "should include nested .rs files"
    );
}

#[test]
fn glob_exclude_md() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec!["*.md".into()]))
        .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".md")));
    assert!(!files.is_empty());
}

#[test]
fn glob_include_and_exclude_interact() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec!["tests/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
    assert!(files.iter().any(|f| f.ends_with(".rs")));
}

#[test]
fn glob_exclude_specific_subdirectory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("vendor")).unwrap();
    fs::write(src.path().join("vendor").join("dep.rs"), "fn dep() {}").unwrap();
    fs::write(src.path().join("root.rs"), "fn root() {}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["vendor/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("vendor/")));
    assert!(files.contains(&"root.rs".to_string()));
}

#[test]
fn glob_multiple_include_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f() {}").unwrap();
    fs::write(src.path().join("config.toml"), "[pkg]").unwrap();
    fs::write(src.path().join("notes.md"), "# Notes").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.rs".into(), "*.toml".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(files.contains(&"config.toml".to_string()));
    assert!(!files.contains(&"notes.md".to_string()));
}

// ===========================================================================
// 3. Git initialisation
// ===========================================================================

#[test]
fn git_init_creates_dot_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert!(ws.path().join(".git").exists());
}

#[test]
fn git_init_creates_baseline_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let log = git(ws.path(), &["log", "--format=%s"]);
    assert!(
        log.contains("baseline"),
        "expected 'baseline' commit, got: {log}"
    );
}

#[test]
fn git_init_exactly_one_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1");
}

#[test]
fn git_working_tree_clean_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty(), "expected clean tree, got: {status}");
}

// ===========================================================================
// 4. .git exclusion – source .git never copied
// ===========================================================================

#[test]
fn source_dot_git_never_copied() {
    let src = tempdir().unwrap();
    let fake_git = src.path().join(".git");
    fs::create_dir_all(fake_git.join("objects")).unwrap();
    fs::write(fake_git.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(fake_git.join("sentinel"), "MUST_NOT_COPY").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    // Stage without git init so we can verify nothing from .git leaks.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws.path().join(".git").exists(),
        "source .git must never be copied"
    );
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn source_dot_git_not_copied_even_with_include_star_star() {
    let src = tempdir().unwrap();
    let fake_git = src.path().join(".git");
    fs::create_dir_all(&fake_git).unwrap();
    fs::write(fake_git.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
}

// ===========================================================================
// 5. Modified files produce meaningful diffs
// ===========================================================================

#[test]
fn modified_file_produces_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original content").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("data.txt"), "modified content").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path()).expect("diff should succeed");
    assert!(diff.contains("data.txt"));
    assert!(diff.contains("modified content"));
    assert!(diff.contains("original content"));
}

#[test]
fn new_file_shows_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.txt"), "hi").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("brand_new.txt"), "I am new").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).expect("status should succeed");
    assert!(status.contains("brand_new.txt"));
    assert!(status.contains("??"), "new file should be untracked");
}

#[test]
fn deleted_file_shows_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "bye").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let status = WorkspaceManager::git_status(ws.path()).expect("status should succeed");
    assert!(status.contains("doomed.txt"));
    assert!(status.contains(" D "));
}

#[test]
fn multiple_mutations_combined_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("mod.txt"), "original").unwrap();
    fs::write(src.path().join("del.txt"), "bye").unwrap();
    fs::write(src.path().join("keep.txt"), "unchanged").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("mod.txt"), "changed").unwrap();
    fs::remove_file(ws.path().join("del.txt")).unwrap();
    fs::write(ws.path().join("add.txt"), "new").unwrap();

    git(ws.path(), &["add", "-A"]);
    let diff = git(ws.path(), &["diff", "--cached", "--no-color"]);

    assert!(diff.contains("mod.txt"));
    assert!(diff.contains("del.txt"));
    assert!(diff.contains("add.txt"));
    assert!(!diff.contains("keep.txt"));
}

// ===========================================================================
// 6. Large directory handling
// ===========================================================================

#[test]
fn large_directory_many_files() {
    let src = tempdir().unwrap();
    let count = 200;
    for i in 0..count {
        fs::write(
            src.path().join(format!("file_{i:04}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files.len(), count, "all {count} files must be staged");
    // Spot-check first and last
    assert_eq!(
        fs::read_to_string(ws.path().join("file_0000.txt")).unwrap(),
        "content 0"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join(format!("file_{:04}.txt", count - 1))).unwrap(),
        format!("content {}", count - 1)
    );
}

#[test]
fn large_single_file() {
    let src = tempdir().unwrap();
    let big = "x".repeat(1024 * 1024); // 1 MiB
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

// ===========================================================================
// 7. Symlink handling
// ===========================================================================

/// Symlinks are intentionally NOT followed (`follow_links(false)` in copy_workspace).
/// Verify that symlinks are skipped without error.
#[test]
fn symlinks_are_skipped_without_error() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();

    // Attempt to create a symlink; skip gracefully on platforms that don't
    // support it (e.g. unprivileged Windows).
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src.path().join("real.txt"), src.path().join("link.txt"))
            .unwrap();
    }
    #[cfg(windows)]
    {
        // Symlinks on Windows require elevated privileges; if creation fails
        // just validate that staging doesn't error and skip the assertion.
        let _ = std::os::windows::fs::symlink_file(
            src.path().join("real.txt"),
            src.path().join("link.txt"),
        );
    }

    // Staging must succeed regardless of symlink presence.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("real.txt").exists());
    // Symlinks are not regular files, so they should be skipped.
}

// ===========================================================================
// 8. Empty directory handling
// ===========================================================================

#[test]
fn empty_source_stages_successfully() {
    let src = tempdir().unwrap();
    // No files at all

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let files = collect_files(ws.path());
    assert!(files.is_empty());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn empty_subdirectories_created_but_no_files() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_child")).unwrap();
    fs::write(src.path().join("root.txt"), "hi").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The directory itself is created (walkdir yields dir entries).
    assert!(ws.path().join("empty_child").exists());
    assert!(ws.path().join("root.txt").exists());
}

// ===========================================================================
// 9. Nested directory preservation
// ===========================================================================

#[test]
fn deeply_nested_directory_preserved() {
    let src = tempdir().unwrap();
    let depth = 10;
    let mut deep = src.path().to_path_buf();
    for i in 0..depth {
        deep = deep.join(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "bottom").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let mut expected = ws.path().to_path_buf();
    for i in 0..depth {
        expected = expected.join(format!("d{i}"));
    }
    assert!(expected.join("leaf.txt").exists());
    assert_eq!(
        fs::read_to_string(expected.join("leaf.txt")).unwrap(),
        "bottom"
    );
}

#[test]
fn parallel_nested_directories() {
    let src = tempdir().unwrap();
    for dir in &["a/b", "c/d", "e/f/g"] {
        let p = src.path().join(dir);
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("file.txt"), *dir).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    for dir in &["a/b", "c/d", "e/f/g"] {
        let staged = ws.path().join(dir).join("file.txt");
        assert!(staged.exists(), "missing {dir}/file.txt");
        assert_eq!(fs::read_to_string(staged).unwrap(), *dir);
    }
}

// ===========================================================================
// 10. Cleanup on drop
// ===========================================================================

#[test]
fn staged_workspace_cleaned_up_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let staged_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists(), "workspace should exist before drop");
    }
    // `ws` dropped here; `TempDir` removed.
    assert!(
        !staged_path.exists(),
        "staged directory should be removed after drop"
    );
}

#[test]
fn stager_workspace_cleaned_up_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

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
    assert!(
        !staged_path.exists(),
        "stager workspace should be removed after drop"
    );
}

// ===========================================================================
// 11. Error handling
// ===========================================================================

#[test]
fn error_nonexistent_source_directory() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist/anywhere")
        .stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("does not exist"),
        "error should mention nonexistence: {msg}"
    );
}

#[test]
fn error_no_source_root_set() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("source_root"),
        "error should mention source_root: {msg}"
    );
}

// ===========================================================================
// 12. Re-staging: stage from an already-staged workspace
// ===========================================================================

#[test]
fn restage_from_already_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("original.txt"), "v1").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Mutate the first staged workspace
    fs::write(ws1.path().join("original.txt"), "v2").unwrap();
    fs::write(ws1.path().join("added.txt"), "new in ws1").unwrap();

    // Re-stage from ws1
    let ws2 = WorkspaceManager::prepare(&staged_spec(ws1.path())).unwrap();

    assert_ne!(ws1.path(), ws2.path());
    // ws2 should see the mutated state of ws1
    assert_eq!(
        fs::read_to_string(ws2.path().join("original.txt")).unwrap(),
        "v2"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("added.txt")).unwrap(),
        "new in ws1"
    );
    // ws2 gets its own clean baseline
    let status = git(ws2.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty(), "re-staged workspace should be clean");
}

#[test]
fn restage_does_not_copy_dot_git_from_first_stage() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // ws1 now has its own .git; re-stage without git init to verify it's not copied.
    let ws2 = WorkspaceStager::new()
        .source_root(ws1.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws2.path().join(".git").exists(),
        ".git from first stage must not be copied into second stage"
    );
    assert!(ws2.path().join("code.rs").exists());
}

// ===========================================================================
// Bonus: PassThrough mode (smoke-test at integration level)
// ===========================================================================

#[test]
fn passthrough_mode_returns_original_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert_eq!(ws.path(), src.path());
}

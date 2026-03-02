// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for workspace staging.
//!
//! Covers: temp directory creation, file copying, .git exclusion, include/exclude
//! glob patterns, git initialization with baseline commit, cleanup on drop,
//! large directory staging, symbolic link handling, empty source directories,
//! nested directory preservation, file permissions (unix), workspace diffs
//! after modifications, multiple simultaneous workspaces, snapshot/diff
//! utilities, template application, change tracking, and operation filtering.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{DiffSummary, diff_workspace};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog};
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

/// Collect sorted relative directory paths (excluding `.git`) under `root`.
fn collect_dirs(root: &Path) -> Vec<String> {
    let mut dirs: Vec<String> = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == ".git"))
        .filter(|e| e.file_type().is_dir())
        .filter_map(|e| {
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel.is_empty() { None } else { Some(rel) }
        })
        .collect();
    dirs.sort();
    dirs
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

/// Create a standard fixture tree with known structure.
fn create_fixture(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src").join("data.json"), r#"{"key":"val"}"#).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests").join("test_one.rs"), "#[test] fn t() {}").unwrap();
}

// ===========================================================================
// 1. Stage workspace creates temp directory
// ===========================================================================

#[test]
fn stage_creates_temp_directory() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "content").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().exists(), "staged workspace path must exist");
    assert!(ws.path().is_dir(), "staged workspace must be a directory");
}

#[test]
fn stage_temp_directory_differs_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "content").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn stager_creates_temp_directory() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("b.txt"), "data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

#[test]
fn stager_path_is_different_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("b.txt"), "data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_ne!(ws.path(), src.path());
}

// ===========================================================================
// 2. Files are copied correctly to staged workspace
// ===========================================================================

#[test]
fn all_files_copied_to_stage() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

#[test]
fn file_contents_preserved_after_staging() {
    let src = tempdir().unwrap();
    let body = "fn main() { println!(\"hello world\"); }";
    fs::write(src.path().join("main.rs"), body).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("main.rs")).unwrap(), body);
}

#[test]
fn binary_file_content_preserved() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("binary.bin"), &data).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(fs::read(ws.path().join("binary.bin")).unwrap(), data);
}

#[test]
fn utf8_file_content_preserved() {
    let src = tempdir().unwrap();
    let content = "日本語テスト 🦀 données ñ";
    fs::write(src.path().join("unicode.txt"), content).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("unicode.txt")).unwrap(),
        content
    );
}

#[test]
fn staging_does_not_modify_source() {
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
// 3. .git directory is excluded by default
// ===========================================================================

#[test]
fn source_dot_git_not_copied() {
    let src = tempdir().unwrap();
    let fake_git = src.path().join(".git");
    fs::create_dir_all(fake_git.join("objects")).unwrap();
    fs::write(fake_git.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(fake_git.join("sentinel"), "MUST_NOT_COPY").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // No .git from source should leak
    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn dot_git_excluded_even_with_wildcard_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
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

#[test]
fn dot_git_excluded_with_manager_prepare() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git").join("refs")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("file.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // The .git that exists is the newly initialized one, not the source
    let log = git(ws.path(), &["log", "--format=%s"]);
    assert!(log.contains("baseline"));
    assert!(ws.path().join("file.txt").exists());
}

#[test]
fn dot_git_contents_never_appear_in_staged_files() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("config"), "[core]").unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    for f in &files {
        assert!(!f.starts_with(".git"), "no .git file should appear: {f}");
    }
}

// ===========================================================================
// 4. Custom include patterns work
// ===========================================================================

#[test]
fn include_only_rs_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.is_empty());
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected file: {f}");
    }
}

#[test]
fn include_multiple_extensions() {
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

#[test]
fn include_specific_directory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("tests")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "pub fn lib(){}").unwrap();
    fs::write(src.path().join("tests").join("t.rs"), "#[test] fn t(){}").unwrap();
    fs::write(src.path().join("README.md"), "# Readme").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.starts_with("src/")));
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
    assert!(!files.contains(&"README.md".to_string()));
}

#[test]
fn include_with_stager_builder() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn keep(){}").unwrap();
    fs::write(src.path().join("skip.txt"), "skip me").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"keep.rs".to_string()));
    assert!(!files.contains(&"skip.txt".to_string()));
}

// ===========================================================================
// 5. Custom exclude patterns work
// ===========================================================================

#[test]
fn exclude_md_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec!["*.md".into()]))
        .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".md")));
    assert!(!files.is_empty());
}

#[test]
fn exclude_subdirectory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("vendor")).unwrap();
    fs::write(src.path().join("vendor").join("dep.rs"), "fn dep(){}").unwrap();
    fs::write(src.path().join("root.rs"), "fn root(){}").unwrap();

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
fn exclude_multiple_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("app.log"), "log data").unwrap();
    fs::write(src.path().join("data.tmp"), "tmp data").unwrap();
    fs::write(src.path().join("code.rs"), "fn code(){}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["*.log".into(), "*.tmp".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".log")));
    assert!(!files.iter().any(|f| f.ends_with(".tmp")));
    assert!(files.contains(&"code.rs".to_string()));
}

#[test]
fn exclude_with_stager_builder() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn keep(){}").unwrap();
    fs::write(src.path().join("skip.log"), "skip me").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"keep.rs".to_string()));
    assert!(!files.contains(&"skip.log".to_string()));
}

// ===========================================================================
// 6. Combined include/exclude patterns
// ===========================================================================

#[test]
fn include_rs_exclude_tests() {
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
fn exclude_overrides_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src").join("generated")).unwrap();
    fs::write(
        src.path().join("src").join("generated").join("out.rs"),
        "generated",
    )
    .unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "fn lib(){}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/generated/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.iter().any(|f| f.contains("generated")));
}

#[test]
fn combined_patterns_with_stager() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("tests").join("fixtures")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "fn lib(){}").unwrap();
    fs::write(
        src.path().join("tests").join("fixtures").join("data.json"),
        "{}",
    )
    .unwrap();
    fs::write(src.path().join("tests").join("unit.rs"), "#[test] fn t(){}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into(), "tests/**".into()])
        .exclude(vec!["tests/fixtures/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"tests/unit.rs".to_string()));
    assert!(!files.iter().any(|f| f.contains("fixtures")));
}

#[test]
fn exclude_everything_results_in_empty_stage() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.rs"), "fn b(){}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

// ===========================================================================
// 7. Staged workspace has git initialized with baseline commit
// ===========================================================================

#[test]
fn git_dot_git_exists_after_staging() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn git_baseline_commit_message() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--format=%s"]);
    assert!(log.contains("baseline"));
}

#[test]
fn git_exactly_one_commit_after_staging() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1");
}

#[test]
fn git_clean_working_tree_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty(), "expected clean tree, got: {status}");
}

#[test]
fn git_all_files_tracked_in_baseline() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let tracked = git(ws.path(), &["ls-files"]);
    assert!(tracked.contains("a.txt"));
    assert!(tracked.contains("b.txt"));
}

#[test]
fn stager_with_git_init_creates_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("x.txt"), "x").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
    let log = git(ws.path(), &["log", "--format=%s"]);
    assert!(log.contains("baseline"));
}

#[test]
fn stager_without_git_init_no_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("x.txt"), "x").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

// ===========================================================================
// 8. Workspace cleanup on drop
// ===========================================================================

#[test]
fn manager_workspace_cleaned_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let staged_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists());
    }
    assert!(
        !staged_path.exists(),
        "staged directory should be removed after drop"
    );
}

#[test]
fn stager_workspace_cleaned_on_drop() {
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
    assert!(!staged_path.exists());
}

#[test]
fn drop_removes_all_contents() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let staged_path;
    let sub_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();
        sub_path = ws.path().join("src").join("utils.rs");
        assert!(sub_path.exists());
    }
    assert!(!staged_path.exists());
    assert!(!sub_path.exists());
}

#[test]
fn passthrough_does_not_clean_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    {
        let ws = WorkspaceManager::prepare(&spec).unwrap();
        assert_eq!(ws.path(), src.path());
    }
    // Source should still exist after drop
    assert!(src.path().join("a.txt").exists());
}

// ===========================================================================
// 9. Large directory staging
// ===========================================================================

#[test]
fn large_directory_200_files() {
    let src = tempdir().unwrap();
    for i in 0..200 {
        fs::write(
            src.path().join(format!("file_{i:04}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 200);
}

#[test]
fn large_single_file_1mb() {
    let src = tempdir().unwrap();
    let big = "x".repeat(1024 * 1024);
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
fn large_directory_content_spot_check() {
    let src = tempdir().unwrap();
    for i in 0..50 {
        fs::write(src.path().join(format!("f{i}.txt")), format!("data-{i}")).unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("f0.txt")).unwrap(),
        "data-0"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("f49.txt")).unwrap(),
        "data-49"
    );
}

#[test]
fn large_nested_directory_tree() {
    let src = tempdir().unwrap();
    for i in 0..20 {
        let dir = src.path().join(format!("dir_{i}"));
        fs::create_dir_all(&dir).unwrap();
        for j in 0..5 {
            fs::write(dir.join(format!("f{j}.txt")), format!("{i}-{j}")).unwrap();
        }
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 100);
}

// ===========================================================================
// 10. Symbolic link handling
// ===========================================================================

#[test]
fn symlinks_skipped_without_error() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real content").unwrap();

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
fn symlink_to_directory_skipped() {
    let src = tempdir().unwrap();
    let real_dir = src.path().join("real_dir");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("file.txt"), "inside").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real_dir, src.path().join("link_dir")).unwrap();
    }
    #[cfg(windows)]
    {
        let _ = std::os::windows::fs::symlink_dir(&real_dir, src.path().join("link_dir"));
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // Real dir and its contents should be present
    assert!(ws.path().join("real_dir").join("file.txt").exists());
}

// ===========================================================================
// 11. Empty source directory staging
// ===========================================================================

#[test]
fn empty_source_stages_successfully() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn empty_source_with_stager() {
    let src = tempdir().unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn empty_subdirectories_preserved() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_child")).unwrap();
    fs::write(src.path().join("root.txt"), "hi").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("empty_child").exists());
    assert!(ws.path().join("root.txt").exists());
}

#[test]
fn only_empty_subdirectories_no_files() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::create_dir_all(src.path().join("c")).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.is_empty());
    assert!(ws.path().join("a").join("b").exists());
    assert!(ws.path().join("c").exists());
}

// ===========================================================================
// 12. Nested directory structure preservation
// ===========================================================================

#[test]
fn deeply_nested_10_levels() {
    let src = tempdir().unwrap();
    let mut deep = src.path().to_path_buf();
    for i in 0..10 {
        deep = deep.join(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "bottom").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let mut expected = ws.path().to_path_buf();
    for i in 0..10 {
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

#[test]
fn mixed_files_and_directories_at_every_level() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "root").unwrap();
    fs::create_dir_all(src.path().join("l1")).unwrap();
    fs::write(src.path().join("l1").join("l1.txt"), "l1").unwrap();
    fs::create_dir_all(src.path().join("l1").join("l2")).unwrap();
    fs::write(src.path().join("l1").join("l2").join("l2.txt"), "l2").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("root.txt")).unwrap(),
        "root"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("l1").join("l1.txt")).unwrap(),
        "l1"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("l1").join("l2").join("l2.txt")).unwrap(),
        "l2"
    );
}

#[test]
fn directory_structure_matches_source() {
    let src = tempdir().unwrap();
    for dir in &["a", "a/b", "c", "c/d/e"] {
        fs::create_dir_all(src.path().join(dir)).unwrap();
        fs::write(src.path().join(dir).join("marker.txt"), format!("in {dir}")).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(collect_dirs(ws.path()), collect_dirs(src.path()));
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

// ===========================================================================
// 13. File permissions preservation (unix)
// ===========================================================================

#[cfg(unix)]
mod unix_permissions {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn executable_permission_preserved() {
        let src = tempdir().unwrap();
        let script = src.path().join("run.sh");
        fs::write(&script, "#!/bin/sh\necho hello").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();

        let staged_perms = fs::metadata(ws.path().join("run.sh"))
            .unwrap()
            .permissions()
            .mode();
        // The copy should preserve the executable bit
        assert!(staged_perms & 0o111 != 0, "executable bit should be set");
    }

    #[test]
    fn readonly_file_copied() {
        let src = tempdir().unwrap();
        let ro = src.path().join("readonly.txt");
        fs::write(&ro, "read only content").unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o444)).unwrap();

        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();

        assert!(ws.path().join("readonly.txt").exists());
        let content = fs::read_to_string(ws.path().join("readonly.txt")).unwrap();
        assert_eq!(content, "read only content");
    }

    #[test]
    fn various_permission_modes() {
        let src = tempdir().unwrap();
        let modes = [
            (0o644, "normal.txt"),
            (0o755, "exec.sh"),
            (0o600, "secret.txt"),
        ];
        for (mode, name) in &modes {
            let path = src.path().join(name);
            fs::write(&path, format!("mode {mode:o}")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(*mode)).unwrap();
        }

        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();

        for (_, name) in &modes {
            assert!(ws.path().join(name).exists());
        }
    }
}

// ===========================================================================
// 14. Workspace diff after modifications
// ===========================================================================

#[test]
fn modified_file_produces_git_diff() {
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
fn new_file_shows_in_git_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.txt"), "hi").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("brand_new.txt"), "I am new").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).expect("status should succeed");
    assert!(status.contains("brand_new.txt"));
}

#[test]
fn deleted_file_shows_in_git_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "bye").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let status = WorkspaceManager::git_status(ws.path()).expect("status should succeed");
    assert!(status.contains("doomed.txt"));
    assert!(status.contains(" D "));
}

#[test]
fn diff_workspace_detects_added_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new_file.txt"), "new content").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary
            .added
            .iter()
            .any(|p| p.to_string_lossy().contains("new_file.txt")),
        "new_file.txt should appear in added: {:?}",
        summary.added
    );
}

#[test]
fn diff_workspace_detects_modified_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "changed").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary
            .modified
            .iter()
            .any(|p| p.to_string_lossy().contains("data.txt")),
        "data.txt should appear in modified: {:?}",
        summary.modified
    );
}

#[test]
fn diff_workspace_detects_deleted_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("gone.txt"), "will be deleted").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("gone.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary
            .deleted
            .iter()
            .any(|p| p.to_string_lossy().contains("gone.txt")),
        "gone.txt should appear in deleted: {:?}",
        summary.deleted
    );
}

#[test]
fn diff_workspace_empty_when_no_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("untouched.txt"), "still here").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty(), "no changes should yield empty diff");
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

#[test]
fn diff_workspace_counts_lines() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "line1\nline2\nline3\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(
        ws.path().join("file.txt"),
        "line1\nchanged\nline3\nnew_line\n",
    )
    .unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_additions > 0);
    assert!(summary.total_deletions > 0);
}

// ===========================================================================
// 15. Multiple simultaneous staged workspaces
// ===========================================================================

#[test]
fn two_workspaces_from_same_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "shared").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_ne!(ws1.path(), ws2.path());
    assert_eq!(
        fs::read_to_string(ws1.path().join("shared.txt")).unwrap(),
        "shared"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "shared"
    );
}

#[test]
fn modifications_in_one_workspace_dont_affect_other() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws1.path().join("data.txt"), "modified in ws1").unwrap();

    assert_eq!(
        fs::read_to_string(ws2.path().join("data.txt")).unwrap(),
        "original",
        "ws2 should not be affected by ws1 modification"
    );
}

#[test]
fn modifications_dont_affect_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "modified").unwrap();

    assert_eq!(
        fs::read_to_string(src.path().join("data.txt")).unwrap(),
        "original",
        "source should not be affected by workspace modification"
    );
}

#[test]
fn three_simultaneous_workspaces() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws3 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let paths: Vec<&Path> = vec![ws1.path(), ws2.path(), ws3.path()];
    // All paths unique
    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            assert_ne!(paths[i], paths[j]);
        }
    }
    // All have the file
    for ws_path in &paths {
        assert!(ws_path.join("file.txt").exists());
    }
}

#[test]
fn drop_one_workspace_keeps_others() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "data").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2_path;
    {
        let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        ws2_path = ws2.path().to_path_buf();
    }
    // ws2 dropped, ws1 still exists
    assert!(ws1.path().exists());
    assert!(!ws2_path.exists());
}

// ===========================================================================
// Additional: Snapshot capture and comparison
// ===========================================================================

#[test]
fn snapshot_capture_counts_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();

    let snap = capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 2);
}

#[test]
fn snapshot_capture_records_sizes() {
    let src = tempdir().unwrap();
    let content = "hello world";
    fs::write(src.path().join("hello.txt"), content).unwrap();

    let snap = capture(src.path()).unwrap();
    assert_eq!(snap.total_size(), content.len() as u64);
}

#[test]
fn snapshot_has_file_check() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("exists.txt"), "yes").unwrap();

    let snap = capture(src.path()).unwrap();
    assert!(snap.has_file(Path::new("exists.txt")));
    assert!(!snap.has_file(Path::new("missing.txt")));
}

#[test]
fn snapshot_get_file_returns_metadata() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let snap = capture(src.path()).unwrap();
    let f = snap.get_file("file.txt").unwrap();
    assert_eq!(f.size, 7);
    assert!(!f.sha256.is_empty());
    assert!(!f.is_binary);
}

#[test]
fn snapshot_compare_identical() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "same").unwrap();

    let snap1 = capture(src.path()).unwrap();
    let snap2 = capture(src.path()).unwrap();

    let diff = compare(&snap1, &snap2);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 1);
}

#[test]
fn snapshot_compare_detects_added() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    let snap1 = capture(dir.path()).unwrap();

    fs::write(dir.path().join("b.txt"), "b").unwrap();
    let snap2 = capture(dir.path()).unwrap();

    let diff = compare(&snap1, &snap2);
    assert_eq!(diff.added.len(), 1);
    assert!(diff.added[0].to_string_lossy().contains("b.txt"));
}

#[test]
fn snapshot_compare_detects_removed() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    let snap1 = capture(dir.path()).unwrap();

    fs::remove_file(dir.path().join("b.txt")).unwrap();
    let snap2 = capture(dir.path()).unwrap();

    let diff = compare(&snap1, &snap2);
    assert_eq!(diff.removed.len(), 1);
    assert!(diff.removed[0].to_string_lossy().contains("b.txt"));
}

#[test]
fn snapshot_compare_detects_modified() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("file.txt"), "original").unwrap();
    let snap1 = capture(dir.path()).unwrap();

    fs::write(dir.path().join("file.txt"), "changed").unwrap();
    let snap2 = capture(dir.path()).unwrap();

    let diff = compare(&snap1, &snap2);
    assert_eq!(diff.modified.len(), 1);
}

#[test]
fn snapshot_staged_workspace_matches_source() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let src_snap = capture(src.path()).unwrap();
    let ws_snap = capture(ws.path()).unwrap();

    let diff = compare(&src_snap, &ws_snap);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), src_snap.file_count());
}

// ===========================================================================
// Additional: Template application into staged workspace
// ===========================================================================

#[test]
fn template_apply_into_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.txt"), "pre-existing").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let mut tmpl = WorkspaceTemplate::new("test", "test template");
    tmpl.add_file("new_from_template.txt", "template content");
    let written = tmpl.apply(ws.path()).unwrap();

    assert_eq!(written, 1);
    assert!(ws.path().join("new_from_template.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("new_from_template.txt")).unwrap(),
        "template content"
    );
}

#[test]
fn template_with_nested_paths() {
    let src = tempdir().unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let mut tmpl = WorkspaceTemplate::new("nested", "nested template");
    tmpl.add_file("a/b/c.txt", "deep content");
    tmpl.add_file("x.txt", "top level");
    let written = tmpl.apply(ws.path()).unwrap();

    assert_eq!(written, 2);
    assert_eq!(
        fs::read_to_string(ws.path().join("a").join("b").join("c.txt")).unwrap(),
        "deep content"
    );
}

#[test]
fn template_registry_management() {
    let mut registry = TemplateRegistry::new();
    assert_eq!(registry.count(), 0);

    let tmpl = WorkspaceTemplate::new("rust-project", "Rust project scaffold");
    registry.register(tmpl);

    assert_eq!(registry.count(), 1);
    assert!(registry.get("rust-project").is_some());
    assert_eq!(registry.list(), vec!["rust-project"]);
}

// ===========================================================================
// Additional: Change tracker
// ===========================================================================

#[test]
fn change_tracker_records_and_summarizes() {
    let mut tracker = ChangeTracker::new();
    assert!(!tracker.has_changes());

    tracker.record(FileChange {
        path: "new.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "mod.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(50),
        size_after: Some(75),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "del.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(30),
        size_after: None,
        content_hash: None,
    });

    assert!(tracker.has_changes());
    let summary = tracker.summary();
    assert_eq!(summary.created, 1);
    assert_eq!(summary.modified, 1);
    assert_eq!(summary.deleted, 1);
    assert_eq!(summary.renamed, 0);
}

#[test]
fn change_tracker_affected_paths() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(5),
        size_after: Some(8),
        content_hash: None,
    });

    let paths = tracker.affected_paths();
    assert_eq!(paths, vec!["a.txt", "b.txt"]);
}

#[test]
fn change_tracker_by_kind() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "c1.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(10),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "c2.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(20),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "m1.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(5),
        size_after: Some(8),
        content_hash: None,
    });

    let created = tracker.by_kind(&ChangeKind::Created);
    assert_eq!(created.len(), 2);
    let modified = tracker.by_kind(&ChangeKind::Modified);
    assert_eq!(modified.len(), 1);
}

#[test]
fn change_tracker_size_delta() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "grow.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(100),
        size_after: Some(200),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "shrink.txt".into(),
        kind: ChangeKind::Modified,
        size_before: Some(300),
        size_after: Some(150),
        content_hash: None,
    });

    let summary = tracker.summary();
    // +100 from grow, -150 from shrink = -50
    assert_eq!(summary.total_size_delta, -50);
}

#[test]
fn change_tracker_clear() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "x.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(1),
        content_hash: None,
    });
    assert!(tracker.has_changes());
    tracker.clear();
    assert!(!tracker.has_changes());
    assert_eq!(tracker.changes().len(), 0);
}

// ===========================================================================
// Additional: Operation log and filter
// ===========================================================================

#[test]
fn operation_log_records_operations() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "b.txt".into(),
        size: 100,
    });
    log.record(FileOperation::Delete {
        path: "c.txt".into(),
    });

    assert_eq!(log.operations().len(), 3);
    assert_eq!(log.reads(), vec!["a.txt"]);
    assert_eq!(log.writes(), vec!["b.txt"]);
    assert_eq!(log.deletes(), vec!["c.txt"]);
}

#[test]
fn operation_log_summary() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "b.txt".into(),
        size: 50,
    });
    log.record(FileOperation::Write {
        path: "c.txt".into(),
        size: 30,
    });

    let summary = log.summary();
    assert_eq!(summary.reads, 1);
    assert_eq!(summary.writes, 2);
    assert_eq!(summary.total_writes_bytes, 80);
}

#[test]
fn operation_log_affected_paths() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "x.txt".into(),
    });
    log.record(FileOperation::Copy {
        from: "x.txt".into(),
        to: "y.txt".into(),
    });

    let paths = log.affected_paths();
    assert!(paths.contains("x.txt"));
    assert!(paths.contains("y.txt"));
}

#[test]
fn operation_filter_allows_all_by_default() {
    let filter = OperationFilter::new();
    assert!(filter.is_allowed("any/path.txt"));
    assert!(filter.is_allowed("another/file.rs"));
}

#[test]
fn operation_filter_with_deny() {
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.log");
    assert!(!filter.is_allowed("app.log"));
    assert!(filter.is_allowed("app.rs"));
}

#[test]
fn operation_filter_with_allow() {
    let mut filter = OperationFilter::new();
    filter.add_allowed_path("src/**");
    assert!(filter.is_allowed("src/lib.rs"));
    assert!(!filter.is_allowed("README.md"));
}

#[test]
fn operation_filter_filters_operations() {
    let mut filter = OperationFilter::new();
    filter.add_denied_path("*.secret");

    let ops = vec![
        FileOperation::Read {
            path: "code.rs".into(),
        },
        FileOperation::Read {
            path: "key.secret".into(),
        },
        FileOperation::Write {
            path: "output.txt".into(),
            size: 10,
        },
    ];

    let allowed = filter.filter_operations(&ops);
    assert_eq!(allowed.len(), 2);
}

// ===========================================================================
// Additional: Error handling
// ===========================================================================

#[test]
fn error_nonexistent_source() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("does not exist"));
}

#[test]
fn error_no_source_root() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("source_root"));
}

// ===========================================================================
// Additional: PassThrough mode
// ===========================================================================

#[test]
fn passthrough_returns_original_path() {
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

// ===========================================================================
// Additional: Re-staging from staged workspace
// ===========================================================================

#[test]
fn restage_from_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("original.txt"), "v1").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws1.path().join("original.txt"), "v2").unwrap();
    fs::write(ws1.path().join("added.txt"), "new in ws1").unwrap();

    let ws2 = WorkspaceManager::prepare(&staged_spec(ws1.path())).unwrap();

    assert_ne!(ws1.path(), ws2.path());
    assert_eq!(
        fs::read_to_string(ws2.path().join("original.txt")).unwrap(),
        "v2"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("added.txt")).unwrap(),
        "new in ws1"
    );
}

#[test]
fn restage_does_not_copy_dot_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(ws1.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws2.path().join(".git").exists());
    assert!(ws2.path().join("code.rs").exists());
}

// ===========================================================================
// Additional: Hidden files and special names
// ===========================================================================

#[test]
fn hidden_files_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    fs::write(src.path().join("visible.txt"), "public").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(".hidden").exists());
    assert!(ws.path().join("visible.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join(".hidden")).unwrap(),
        "secret"
    );
}

#[test]
fn dotfiles_except_git_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    fs::write(src.path().join(".env"), "SECRET=x").unwrap();
    fs::create_dir_all(src.path().join(".config")).unwrap();
    fs::write(src.path().join(".config").join("settings.json"), "{}").unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(".gitignore").exists());
    assert!(ws.path().join(".env").exists());
    assert!(ws.path().join(".config").join("settings.json").exists());
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn files_with_spaces_in_names() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("my file.txt"), "space").unwrap();
    fs::create_dir_all(src.path().join("my dir")).unwrap();
    fs::write(src.path().join("my dir").join("inner file.txt"), "inner").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("my file.txt")).unwrap(),
        "space"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("my dir").join("inner file.txt")).unwrap(),
        "inner"
    );
}

#[test]
fn files_with_unicode_names() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("données.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("données.txt")).unwrap(),
        "data"
    );
}

// ===========================================================================
// Additional: WorkspaceStager default behavior
// ===========================================================================

#[test]
fn stager_default_has_git_init_enabled() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_default_has_no_globs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "fn a(){}").unwrap();
    fs::write(src.path().join("b.txt"), "text").unwrap();
    fs::write(src.path().join("c.md"), "# markdown").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files.len(), 3);
}

#[test]
fn stager_default_trait() {
    let stager = WorkspaceStager::default();
    // Should be equivalent to WorkspaceStager::new()
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = stager.source_root(src.path()).stage().unwrap();
    assert!(ws.path().join(".git").exists());
}

// ===========================================================================
// Additional: DiffSummary struct methods
// ===========================================================================

#[test]
fn diff_summary_is_empty() {
    let summary = DiffSummary::default();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

#[test]
fn diff_summary_file_count() {
    let summary = DiffSummary {
        added: vec![PathBuf::from("a.txt")],
        modified: vec![PathBuf::from("b.txt"), PathBuf::from("c.txt")],
        deleted: vec![PathBuf::from("d.txt")],
        total_additions: 10,
        total_deletions: 5,
    };
    assert!(!summary.is_empty());
    assert_eq!(summary.file_count(), 4);
    assert_eq!(summary.total_changes(), 15);
}

// ===========================================================================
// Additional: Empty file handling
// ===========================================================================

#[test]
fn empty_file_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("empty.txt").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

#[test]
fn mixed_empty_and_nonempty_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();
    fs::write(src.path().join("content.txt"), "has content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
    assert_eq!(
        fs::read_to_string(ws.path().join("content.txt")).unwrap(),
        "has content"
    );
}

// ===========================================================================
// Additional: Glob edge cases
// ===========================================================================

#[test]
fn glob_with_no_matching_include_produces_empty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.nonexistent".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn glob_exclude_with_no_matching_files_keeps_all() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["*.nonexistent".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files.len(), 2);
}

#[test]
fn glob_double_star_matches_deep_nesting() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    fs::write(
        src.path().join("a").join("b").join("c").join("deep.rs"),
        "fn deep(){}",
    )
    .unwrap();
    fs::write(src.path().join("top.rs"), "fn top(){}").unwrap();
    fs::write(src.path().join("doc.md"), "# Doc").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("deep.rs")));
    assert!(files.contains(&"top.rs".to_string()));
    assert!(!files.iter().any(|f| f.ends_with(".md")));
}

// ===========================================================================
// Additional: Workspace with many file types
// ===========================================================================

#[test]
fn workspace_with_diverse_file_types() {
    let src = tempdir().unwrap();
    let extensions = ["rs", "txt", "md", "json", "toml", "yaml", "py", "js"];
    for ext in &extensions {
        fs::write(
            src.path().join(format!("file.{ext}")),
            format!("content for {ext}"),
        )
        .unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files.len(), extensions.len());
    for ext in &extensions {
        assert_eq!(
            fs::read_to_string(ws.path().join(format!("file.{ext}"))).unwrap(),
            format!("content for {ext}")
        );
    }
}

// ===========================================================================
// Additional: ChangeKind rename variant
// ===========================================================================

#[test]
fn change_tracker_rename() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "new_name.txt".into(),
        kind: ChangeKind::Renamed {
            from: "old_name.txt".into(),
        },
        size_before: Some(10),
        size_after: Some(10),
        content_hash: None,
    });

    let summary = tracker.summary();
    assert_eq!(summary.renamed, 1);
    assert_eq!(summary.total_size_delta, 0);
}

// ===========================================================================
// Additional: Operation log move/copy/createdir
// ===========================================================================

#[test]
fn operation_log_move_and_copy() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Move {
        from: "old.txt".into(),
        to: "new.txt".into(),
    });
    log.record(FileOperation::Copy {
        from: "src.txt".into(),
        to: "dst.txt".into(),
    });
    log.record(FileOperation::CreateDir {
        path: "new_dir".into(),
    });

    let summary = log.summary();
    assert_eq!(summary.moves, 1);
    assert_eq!(summary.copies, 1);
    assert_eq!(summary.create_dirs, 1);
}

#[test]
fn operation_log_clear() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    assert_eq!(log.operations().len(), 1);
    log.clear();
    assert_eq!(log.operations().len(), 0);
}

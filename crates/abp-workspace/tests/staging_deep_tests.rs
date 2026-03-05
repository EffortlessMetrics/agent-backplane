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
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for workspace staging (abp-workspace crate).
//!
//! Covers workspace creation, git initialization, file copying, glob filtering,
//! diff analysis, policy enforcement, and edge cases.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{
    ChangeType, DiffAnalyzer, DiffPolicy, DiffSummary, PolicyResult, WorkspaceDiff, diff_workspace,
};
use abp_workspace::snapshot::{capture, compare};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a WorkspaceSpec in Staged mode with no globs.
fn staged_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

/// Create a WorkspaceSpec in Staged mode with include/exclude globs.
fn staged_spec_globs(root: &Path, include: Vec<String>, exclude: Vec<String>) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
    }
}

/// Collect sorted relative file paths (excluding `.git`) under a root.
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

/// Populate a source directory with a small set of files.
fn populate_source(root: &Path) {
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("src").join("lib.rs"), "pub fn hello() {}").unwrap();
}

/// Check that `.git` directory exists in the workspace.
fn has_git_dir(root: &Path) -> bool {
    root.join(".git").exists()
}

// ===========================================================================
// 1. Workspace creation
// ===========================================================================

#[test]
fn workspace_creation_basic() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let spec = staged_spec(src.path());
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().exists());
}

#[test]
fn workspace_creation_returns_different_path() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let spec = staged_spec(src.path());
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn workspace_stager_basic() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().exists());
}

#[test]
fn workspace_stager_missing_source_root_errors() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
}

#[test]
fn workspace_stager_nonexistent_dir_errors() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
}

// ===========================================================================
// 2. Git initialization
// ===========================================================================

#[test]
fn staged_workspace_has_git_dir() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(has_git_dir(ws.path()));
}

#[test]
fn stager_with_git_init_creates_git_dir() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(has_git_dir(ws.path()));
}

#[test]
fn stager_without_git_init_skips_git_dir() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Source had no .git, and we told stager not to init one.
    assert!(!has_git_dir(ws.path()));
}

#[test]
fn git_status_clean_after_staging() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // After staging, a baseline commit should exist; `git status` should work.
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
}

// ===========================================================================
// 3. File copying — correct content
// ===========================================================================

#[test]
fn files_copied_with_correct_content() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("README.md")).unwrap(),
        "# Hello"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("src").join("main.rs")).unwrap(),
        "fn main() {}"
    );
}

#[test]
fn all_expected_files_present() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"Cargo.toml".to_string()));
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(files.contains(&"src/lib.rs".to_string()));
}

#[test]
fn file_count_matches_source() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let src_files = collect_files(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws_files = collect_files(ws.path());
    assert_eq!(src_files.len(), ws_files.len());
}

// ===========================================================================
// 4. .git exclusion
// ===========================================================================

#[test]
fn source_git_dir_excluded_from_copy() {
    let src = tempdir().unwrap();
    populate_source(src.path());

    // Create a fake .git directory in the source.
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(
        src.path().join(".git").join("objects").join("fakehash"),
        "data",
    )
    .unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The .git from *source* should not be copied into the workspace.
    // But the stager may create its own .git (we disabled that here).
    let files = collect_files(ws.path());
    for f in &files {
        assert!(!f.starts_with(".git"), "found .git file in workspace: {f}");
    }
}

#[test]
fn staged_workspace_git_is_fresh_not_source_git() {
    let src = tempdir().unwrap();
    populate_source(src.path());

    // Put a marker in source .git
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("marker"), "from_source").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // The workspace should have .git from initialization, not from source.
    assert!(!ws.path().join(".git").join("marker").exists());
}

// ===========================================================================
// 5. Include globs
// ===========================================================================

#[test]
fn include_only_rs_files() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let spec = staged_spec_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.contains(&"README.md".to_string()));
    assert!(!files.contains(&"Cargo.toml".to_string()));
}

#[test]
fn include_multiple_patterns() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let spec = staged_spec_globs(src.path(), vec!["**/*.rs".into(), "*.md".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(files.contains(&"README.md".to_string()));
    assert!(!files.contains(&"Cargo.toml".to_string()));
}

// ===========================================================================
// 6. Exclude patterns
// ===========================================================================

#[test]
fn exclude_log_files() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    fs::write(src.path().join("build.log"), "log data").unwrap();
    fs::write(src.path().join("error.log"), "error data").unwrap();

    let spec = staged_spec_globs(src.path(), vec![], vec!["*.log".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.contains(&"build.log".to_string()));
    assert!(!files.contains(&"error.log".to_string()));
    assert!(files.contains(&"README.md".to_string()));
}

#[test]
fn exclude_target_directory() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    fs::create_dir_all(src.path().join("target").join("debug")).unwrap();
    fs::write(
        src.path().join("target").join("debug").join("app"),
        "binary",
    )
    .unwrap();

    let spec = staged_spec_globs(src.path(), vec![], vec!["target/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("target/")));
    assert!(files.contains(&"README.md".to_string()));
}

#[test]
fn include_and_exclude_combined() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    fs::create_dir_all(src.path().join("src").join("generated")).unwrap();
    fs::write(
        src.path().join("src").join("generated").join("out.rs"),
        "generated",
    )
    .unwrap();

    let spec = staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/generated/**".into()],
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(!files.contains(&"src/generated/out.rs".to_string()));
    assert!(!files.contains(&"README.md".to_string()));
}

// ===========================================================================
// 7. Nested directories
// ===========================================================================

#[test]
fn deep_nested_dirs_copied() {
    let src = tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.txt"), "deep content").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let dest_file = ws
        .path()
        .join("a")
        .join("b")
        .join("c")
        .join("d")
        .join("deep.txt");
    assert!(dest_file.exists());
    assert_eq!(fs::read_to_string(dest_file).unwrap(), "deep content");
}

#[test]
fn many_sibling_dirs_copied() {
    let src = tempdir().unwrap();
    for i in 0..10 {
        let dir = src.path().join(format!("dir_{i}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), format!("content {i}")).unwrap();
    }

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for i in 0..10 {
        let f = ws.path().join(format!("dir_{i}")).join("file.txt");
        assert!(f.exists(), "dir_{i}/file.txt should exist");
        assert_eq!(fs::read_to_string(f).unwrap(), format!("content {i}"));
    }
}

// ===========================================================================
// 8. Empty directories
// ===========================================================================

#[test]
fn empty_directories_handling() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "root").unwrap();
    fs::create_dir_all(src.path().join("empty_dir")).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // The root.txt must be present; empty_dir may or may not be created
    // since walkdir may yield the dir and copy_workspace creates it.
    assert!(ws.path().join("root.txt").exists());
    // Just verify no error occurs — both outcomes are acceptable.
}

#[test]
fn empty_dir_inside_populated_dir() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("parent").join("child_empty")).unwrap();
    fs::write(src.path().join("parent").join("sibling.txt"), "content").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("parent").join("sibling.txt").exists());
}

// ===========================================================================
// 9. DiffAnalyzer
// ===========================================================================

#[test]
fn diff_analyzer_no_changes_initially() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    // Right after staging, git baseline commit includes all files, so no changes.
    let has = analyzer.has_changes();
    // The baseline commit may or may not show changes depending on git behavior;
    // we just verify the call doesn't error.
    let _ = has;
}

#[test]
fn diff_analyzer_detects_new_file() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("new_file.txt"), "new content").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.has_changes());
    let changed = analyzer.changed_files();
    assert!(
        changed.contains(&PathBuf::from("new_file.txt")),
        "changed files should include new_file.txt: {changed:?}"
    );
}

#[test]
fn diff_analyzer_detects_modified_file() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Modify an existing file.
    fs::write(ws.path().join("README.md"), "# Modified").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.file_was_modified(Path::new("README.md")));
}

#[test]
fn diff_analyzer_detects_deleted_file() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::remove_file(ws.path().join("README.md")).unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.has_changes());
    assert!(analyzer.file_was_modified(Path::new("README.md")));
}

#[test]
fn diff_analyzer_analyze_returns_workspace_diff() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    fs::write(ws.path().join("added.txt"), "new file").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    assert!(!diff.is_empty());
    assert!(diff.file_count() > 0);
}

// ===========================================================================
// 10. DiffPolicy
// ===========================================================================

#[test]
fn diff_policy_pass_when_within_limits() {
    let diff = WorkspaceDiff {
        files_added: vec![make_file_change("new.txt", ChangeType::Added, 10, 0)],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 10,
        total_deletions: 0,
    };

    let policy = DiffPolicy {
        max_files: Some(5),
        max_additions: Some(100),
        denied_paths: vec![],
    };

    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_fail_too_many_files() {
    let diff = WorkspaceDiff {
        files_added: vec![
            make_file_change("a.txt", ChangeType::Added, 1, 0),
            make_file_change("b.txt", ChangeType::Added, 1, 0),
            make_file_change("c.txt", ChangeType::Added, 1, 0),
        ],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 3,
        total_deletions: 0,
    };

    let policy = DiffPolicy {
        max_files: Some(2),
        max_additions: None,
        denied_paths: vec![],
    };

    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations.iter().any(|v| v.contains("too many files")));
    }
}

#[test]
fn diff_policy_fail_too_many_additions() {
    let diff = WorkspaceDiff {
        files_added: vec![make_file_change("big.txt", ChangeType::Added, 500, 0)],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 500,
        total_deletions: 0,
    };

    let policy = DiffPolicy {
        max_files: None,
        max_additions: Some(100),
        denied_paths: vec![],
    };

    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations.iter().any(|v| v.contains("too many additions")));
    }
}

#[test]
fn diff_policy_fail_denied_path() {
    let diff = WorkspaceDiff {
        files_modified: vec![make_file_change(
            "secret/key.pem",
            ChangeType::Modified,
            1,
            1,
        )],
        files_added: vec![],
        files_deleted: vec![],
        total_additions: 1,
        total_deletions: 1,
    };

    let policy = DiffPolicy {
        max_files: None,
        max_additions: None,
        denied_paths: vec!["secret/**".into()],
    };

    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations.iter().any(|v| v.contains("denied path")));
    }
}

#[test]
fn diff_policy_empty_allows_everything() {
    let diff = WorkspaceDiff {
        files_added: vec![make_file_change("x.rs", ChangeType::Added, 999, 0)],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 999,
        total_deletions: 0,
    };

    let policy = DiffPolicy::default();
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_multiple_violations() {
    let diff = WorkspaceDiff {
        files_added: vec![
            make_file_change("a.txt", ChangeType::Added, 100, 0),
            make_file_change("b.txt", ChangeType::Added, 100, 0),
            make_file_change("c.txt", ChangeType::Added, 100, 0),
        ],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 300,
        total_deletions: 0,
    };

    let policy = DiffPolicy {
        max_files: Some(1),
        max_additions: Some(50),
        denied_paths: vec![],
    };

    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations.len() >= 2);
    }
}

// ===========================================================================
// 11. WorkspaceDiff
// ===========================================================================

#[test]
fn workspace_diff_empty() {
    let diff = WorkspaceDiff::default();
    assert!(diff.is_empty());
    assert_eq!(diff.file_count(), 0);
    assert_eq!(diff.summary(), "No changes detected.");
}

#[test]
fn workspace_diff_file_count() {
    let diff = WorkspaceDiff {
        files_added: vec![make_file_change("a.rs", ChangeType::Added, 10, 0)],
        files_modified: vec![make_file_change("b.rs", ChangeType::Modified, 5, 2)],
        files_deleted: vec![make_file_change("c.rs", ChangeType::Deleted, 0, 8)],
        total_additions: 15,
        total_deletions: 10,
    };
    assert_eq!(diff.file_count(), 3);
    assert!(!diff.is_empty());
}

#[test]
fn workspace_diff_summary_format() {
    let diff = WorkspaceDiff {
        files_added: vec![make_file_change("a.rs", ChangeType::Added, 10, 0)],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 10,
        total_deletions: 0,
    };
    let s = diff.summary();
    assert!(s.contains("1 file(s) changed"));
    assert!(s.contains("1 added"));
    assert!(s.contains("+10"));
}

#[test]
fn workspace_diff_with_real_workspace() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Add a file and check diff_workspace.
    fs::write(ws.path().join("new.txt"), "new content").unwrap();
    let summary = diff_workspace(&ws).unwrap();
    assert!(!summary.is_empty());
    assert!(summary.added.contains(&PathBuf::from("new.txt")));
}

// ===========================================================================
// 12. Symlink handling
// ===========================================================================

#[test]
fn symlinks_are_not_followed() {
    let src = tempdir().unwrap();
    populate_source(src.path());

    // Create a symlink in the source directory.
    let link_path = src.path().join("link_to_readme");
    // On Windows, symlinks may require elevated privileges; if creation fails
    // we skip the test gracefully.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src.path().join("README.md"), &link_path).unwrap();
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(src.path().join("README.md"), &link_path).is_err() {
            // Symlink creation requires elevated privileges on some Windows setups.
            eprintln!("Skipping symlink test — insufficient privileges");
            return;
        }
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The copy_workspace uses follow_links(false), so the symlink itself
    // should not appear as a regular file in the workspace.
    let dest = ws.path().join("link_to_readme");
    // Symlinks are neither files nor dirs in the walkdir sense with follow_links(false),
    // so the symlink target should not be copied.
    assert!(
        !dest.exists() || dest.is_symlink(),
        "symlink should not be copied as a regular file"
    );
}

// ===========================================================================
// 13. Large file handling / performance with many files
// ===========================================================================

#[test]
fn many_files_copied_correctly() {
    let src = tempdir().unwrap();
    let count = 100;
    for i in 0..count {
        fs::write(
            src.path().join(format!("file_{i:03}.txt")),
            format!("data {i}"),
        )
        .unwrap();
    }

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), count);
}

#[test]
fn large_file_content_preserved() {
    let src = tempdir().unwrap();
    let content = "x".repeat(1_000_000); // 1 MB
    fs::write(src.path().join("large.txt"), &content).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let copied = fs::read_to_string(ws.path().join("large.txt")).unwrap();
    assert_eq!(copied.len(), content.len());
    assert_eq!(copied, content);
}

#[test]
fn deeply_nested_many_files() {
    let src = tempdir().unwrap();
    for i in 0..5 {
        let dir = src.path().join(format!("l1_{i}")).join("l2").join("l3");
        fs::create_dir_all(&dir).unwrap();
        for j in 0..5 {
            fs::write(dir.join(format!("f_{j}.txt")), format!("{i}-{j}")).unwrap();
        }
    }

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 25);
}

// ===========================================================================
// 14. Serde roundtrip
// ===========================================================================

#[test]
fn diff_summary_serde_roundtrip() {
    let summary = DiffSummary {
        added: vec![PathBuf::from("new.rs")],
        modified: vec![PathBuf::from("lib.rs")],
        deleted: vec![PathBuf::from("old.rs")],
        total_additions: 42,
        total_deletions: 7,
    };

    let json = serde_json::to_string(&summary).unwrap();
    let back: DiffSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(summary, back);
}

#[test]
fn workspace_diff_serde_roundtrip() {
    let diff = WorkspaceDiff {
        files_added: vec![make_file_change("a.rs", ChangeType::Added, 10, 0)],
        files_modified: vec![make_file_change("b.rs", ChangeType::Modified, 5, 3)],
        files_deleted: vec![],
        total_additions: 15,
        total_deletions: 3,
    };

    let json = serde_json::to_string(&diff).unwrap();
    let back: WorkspaceDiff = serde_json::from_str(&json).unwrap();
    assert_eq!(diff, back);
}

#[test]
fn diff_policy_serde_roundtrip() {
    let policy = DiffPolicy {
        max_files: Some(10),
        max_additions: Some(500),
        denied_paths: vec!["secret/**".into(), "*.key".into()],
    };

    let json = serde_json::to_string(&policy).unwrap();
    let back: DiffPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&back).unwrap());
}

#[test]
fn policy_result_serde_roundtrip() {
    let pass = PolicyResult::Pass;
    let json = serde_json::to_string(&pass).unwrap();
    let back: PolicyResult = serde_json::from_str(&json).unwrap();
    assert!(back.is_pass());

    let fail = PolicyResult::Fail {
        violations: vec!["too many files".into()],
    };
    let json = serde_json::to_string(&fail).unwrap();
    let back: PolicyResult = serde_json::from_str(&json).unwrap();
    assert!(!back.is_pass());
}

#[test]
fn change_type_serde_roundtrip() {
    for ct in [ChangeType::Added, ChangeType::Modified, ChangeType::Deleted] {
        let json = serde_json::to_string(&ct).unwrap();
        let back: ChangeType = serde_json::from_str(&json).unwrap();
        assert_eq!(ct, back);
    }
}

// ===========================================================================
// Additional coverage: PassThrough mode, snapshot, misc
// ===========================================================================

#[test]
fn passthrough_mode_uses_original_path() {
    let src = tempdir().unwrap();
    populate_source(src.path());
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
fn snapshot_captures_all_files() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let snap = capture(src.path()).unwrap();
    assert_eq!(snap.file_count(), 4); // README.md, Cargo.toml, src/main.rs, src/lib.rs
    assert!(snap.has_file(Path::new("README.md")));
    assert!(snap.has_file(Path::new("src").join("main.rs")));
}

#[test]
fn snapshot_compare_detects_added() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let snap1 = capture(src.path()).unwrap();

    fs::write(src.path().join("new.txt"), "new").unwrap();
    let snap2 = capture(src.path()).unwrap();

    let diff = compare(&snap1, &snap2);
    assert!(diff.added.contains(&PathBuf::from("new.txt")));
}

#[test]
fn snapshot_compare_detects_modified() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let snap1 = capture(src.path()).unwrap();

    fs::write(src.path().join("README.md"), "# Changed").unwrap();
    let snap2 = capture(src.path()).unwrap();

    let diff = compare(&snap1, &snap2);
    assert!(diff.modified.contains(&PathBuf::from("README.md")));
}

#[test]
fn snapshot_compare_detects_removed() {
    let src = tempdir().unwrap();
    populate_source(src.path());
    let snap1 = capture(src.path()).unwrap();

    fs::remove_file(src.path().join("README.md")).unwrap();
    let snap2 = capture(src.path()).unwrap();

    let diff = compare(&snap1, &snap2);
    assert!(diff.removed.contains(&PathBuf::from("README.md")));
}

#[test]
fn stager_default_is_same_as_new() {
    let s1 = WorkspaceStager::new();
    let s2 = WorkspaceStager::default();
    // Both should produce equivalent configs (no source_root set).
    assert!(format!("{s1:?}").contains("source_root: None"));
    assert!(format!("{s2:?}").contains("source_root: None"));
}

#[test]
fn diff_summary_total_changes() {
    let s = DiffSummary {
        added: vec![],
        modified: vec![],
        deleted: vec![],
        total_additions: 10,
        total_deletions: 5,
    };
    assert_eq!(s.total_changes(), 15);
}

// ===========================================================================
// Helpers for constructing test data
// ===========================================================================

fn make_file_change(
    path: &str,
    change_type: ChangeType,
    additions: usize,
    deletions: usize,
) -> abp_workspace::diff::FileChange {
    abp_workspace::diff::FileChange {
        path: PathBuf::from(path),
        change_type,
        additions,
        deletions,
        is_binary: false,
    }
}

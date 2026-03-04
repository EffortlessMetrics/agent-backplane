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
//! Integration tests for the workspace staging and diff analysis pipeline.

use std::fs;
use std::path::{Path, PathBuf};

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{ChangeType, DiffAnalyzer, DiffPolicy, PolicyResult, WorkspaceDiff};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temporary source directory with the given file tree.
/// Each entry is `(relative_path, contents)`.
fn source_dir(files: &[(&str, &str)]) -> TempDir {
    let tmp = tempfile::tempdir().expect("create temp source dir");
    for (path, contents) in files {
        let full = tmp.path().join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&full, contents).expect("write file");
    }
    tmp
}

/// Helper: stage a source dir through [`WorkspaceStager`] with default settings.
fn stage_default(src: &Path) -> abp_workspace::PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .stage()
        .expect("staging should succeed")
}

/// Helper: list all files under `root` as sorted relative paths (forward-slash
/// separated for platform-independent comparison).
fn list_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
    {
        let entry = entry.expect("walk entry");
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(root)
                .expect("strip prefix")
                .to_string_lossy()
                .replace('\\', "/");
            files.push(rel);
        }
    }
    files.sort();
    files
}

// ===========================================================================
// Module: staging_lifecycle
// ===========================================================================
mod staging_lifecycle {
    use super::*;

    #[test]
    fn stage_empty_directory() {
        let src = tempfile::tempdir().unwrap();
        let ws = stage_default(src.path());
        assert!(ws.path().exists(), "workspace directory should exist");
    }

    #[test]
    fn stage_directory_with_files() {
        let src = source_dir(&[("a.txt", "aaa"), ("b.txt", "bbb")]);
        let ws = stage_default(src.path());
        let files = list_files(ws.path());
        assert!(files.contains(&"a.txt".to_string()));
        assert!(files.contains(&"b.txt".to_string()));
    }

    #[test]
    fn stage_with_include_globs() {
        let src = source_dir(&[("keep.rs", "fn main(){}"), ("skip.txt", "nope")]);
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .include(vec!["*.rs".into()])
            .stage()
            .unwrap();
        let files = list_files(ws.path());
        assert!(files.contains(&"keep.rs".to_string()));
        assert!(!files.contains(&"skip.txt".to_string()));
    }

    #[test]
    fn stage_with_exclude_globs() {
        let src = source_dir(&[("keep.rs", "fn main(){}"), ("skip.log", "logdata")]);
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .exclude(vec!["*.log".into()])
            .stage()
            .unwrap();
        let files = list_files(ws.path());
        assert!(files.contains(&"keep.rs".to_string()));
        assert!(!files.contains(&"skip.log".to_string()));
    }

    #[test]
    fn stage_excludes_dot_git_by_default() {
        let src = source_dir(&[("file.txt", "hello")]);
        // Plant a fake .git dir in the source.
        fs::create_dir_all(src.path().join(".git")).unwrap();
        fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();

        let ws = stage_default(src.path());
        // The staged workspace will have its OWN .git (from ensure_git_repo),
        // but the source's fake .git files should NOT have been copied.
        // Verify the baseline commit exists (from ensure_git_repo, not from source).
        let log = std::process::Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(ws.path())
            .output()
            .expect("git log");
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("baseline"),
            "git should be from ensure_git_repo, not copied from source"
        );
        // The original source file should still be staged.
        assert!(ws.path().join("file.txt").exists());
    }

    #[test]
    fn staged_workspace_has_git_initialized() {
        let src = source_dir(&[("readme.md", "# Hello")]);
        let ws = stage_default(src.path());
        assert!(
            ws.path().join(".git").exists(),
            "staged workspace should have .git"
        );
    }

    #[test]
    fn staged_workspace_has_baseline_commit() {
        let src = source_dir(&[("readme.md", "# Hello")]);
        let ws = stage_default(src.path());

        let output = std::process::Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(ws.path())
            .output()
            .expect("git log");
        let log = String::from_utf8_lossy(&output.stdout);
        assert!(
            log.contains("baseline"),
            "should have a baseline commit, got: {log}"
        );
    }

    #[test]
    fn stage_preserves_file_contents_exactly() {
        let content = "line 1\nline 2\nspecial chars: éàü 日本語\n";
        let src = source_dir(&[("data.txt", content)]);
        let ws = stage_default(src.path());
        let staged = fs::read_to_string(ws.path().join("data.txt")).unwrap();
        assert_eq!(staged, content);
    }

    #[test]
    fn stage_preserves_directory_structure() {
        let src = source_dir(&[
            ("src/main.rs", "fn main(){}"),
            ("src/lib.rs", "pub mod foo;"),
            ("tests/test.rs", "#[test] fn t(){}"),
        ]);
        let ws = stage_default(src.path());
        assert!(ws.path().join("src/main.rs").is_file());
        assert!(ws.path().join("src/lib.rs").is_file());
        assert!(ws.path().join("tests/test.rs").is_file());
    }

    #[test]
    fn stage_with_nested_directories() {
        let src = source_dir(&[
            ("a/b/c/deep.txt", "deep"),
            ("a/b/mid.txt", "mid"),
            ("a/top.txt", "top"),
        ]);
        let ws = stage_default(src.path());
        let files = list_files(ws.path());
        assert!(files.contains(&"a/b/c/deep.txt".to_string()));
        assert!(files.contains(&"a/b/mid.txt".to_string()));
        assert!(files.contains(&"a/top.txt".to_string()));
    }

    #[test]
    fn stage_via_workspace_manager() {
        let src = source_dir(&[("hello.txt", "world")]);
        let spec = WorkspaceSpec {
            root: src.path().to_string_lossy().into_owned(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let ws = WorkspaceManager::prepare(&spec).unwrap();
        assert!(ws.path().join("hello.txt").exists());
        assert!(ws.path().join(".git").exists());
    }
}

// ===========================================================================
// Module: diff_lifecycle
// ===========================================================================
mod diff_lifecycle {
    use super::*;

    #[test]
    fn new_workspace_has_no_changes() {
        let src = source_dir(&[("file.txt", "content")]);
        let ws = stage_default(src.path());
        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();
        assert!(diff.is_empty(), "fresh workspace should have no changes");
    }

    #[test]
    fn add_file_detected_as_added() {
        let src = source_dir(&[("original.txt", "original")]);
        let ws = stage_default(src.path());

        fs::write(ws.path().join("new_file.txt"), "new content").unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();
        assert_eq!(diff.files_added.len(), 1);
        assert_eq!(diff.files_added[0].path, PathBuf::from("new_file.txt"));
        assert_eq!(diff.files_added[0].change_type, ChangeType::Added);
    }

    #[test]
    fn modify_file_detected_as_modified() {
        let src = source_dir(&[("file.txt", "original")]);
        let ws = stage_default(src.path());

        fs::write(ws.path().join("file.txt"), "modified content\nextra line").unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();
        assert_eq!(diff.files_modified.len(), 1);
        assert_eq!(diff.files_modified[0].path, PathBuf::from("file.txt"));
        assert_eq!(diff.files_modified[0].change_type, ChangeType::Modified);
    }

    #[test]
    fn delete_file_detected_as_deleted() {
        let src = source_dir(&[("doomed.txt", "will be deleted")]);
        let ws = stage_default(src.path());

        fs::remove_file(ws.path().join("doomed.txt")).unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();
        assert_eq!(diff.files_deleted.len(), 1);
        assert_eq!(diff.files_deleted[0].path, PathBuf::from("doomed.txt"));
        assert_eq!(diff.files_deleted[0].change_type, ChangeType::Deleted);
    }

    #[test]
    fn multiple_changes_detected_correctly() {
        let src = source_dir(&[
            ("keep.txt", "keep"),
            ("modify.txt", "old"),
            ("delete.txt", "bye"),
        ]);
        let ws = stage_default(src.path());

        fs::write(ws.path().join("added.txt"), "new").unwrap();
        fs::write(ws.path().join("modify.txt"), "new content").unwrap();
        fs::remove_file(ws.path().join("delete.txt")).unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();
        assert_eq!(diff.files_added.len(), 1);
        assert_eq!(diff.files_modified.len(), 1);
        assert_eq!(diff.files_deleted.len(), 1);
        assert_eq!(diff.file_count(), 3);
    }

    #[test]
    fn diff_summary_is_human_readable() {
        let src = source_dir(&[("a.txt", "aaa")]);
        let ws = stage_default(src.path());

        fs::write(ws.path().join("b.txt"), "bbb\nccc\n").unwrap();
        fs::write(ws.path().join("a.txt"), "modified\n").unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();
        let summary = diff.summary();
        assert!(summary.contains("file(s) changed"), "got: {summary}");
        assert!(summary.contains("added"), "got: {summary}");
        assert!(summary.contains("modified"), "got: {summary}");
    }

    #[test]
    fn binary_files_detected() {
        let src = source_dir(&[("readme.txt", "hello")]);
        let ws = stage_default(src.path());

        // Write a file with null bytes to trigger binary detection.
        let binary_content: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0x00, 0xFF, 0xFE];
        fs::write(ws.path().join("image.bin"), &binary_content).unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();
        assert_eq!(diff.files_added.len(), 1);
        assert!(
            diff.files_added[0].is_binary,
            "file with null bytes should be detected as binary"
        );
    }

    #[test]
    fn empty_diff_returns_empty_workspace_diff() {
        let diff = WorkspaceDiff::default();
        assert!(diff.is_empty());
        assert_eq!(diff.file_count(), 0);
        assert_eq!(diff.total_additions, 0);
        assert_eq!(diff.total_deletions, 0);
        assert_eq!(diff.summary(), "No changes detected.");
    }
}

// ===========================================================================
// Module: diff_policy_enforcement
// ===========================================================================
mod diff_policy_enforcement {
    use super::*;

    fn make_diff(added: usize, modified: usize, deleted: usize, additions: usize) -> WorkspaceDiff {
        use abp_workspace::diff::FileChange;
        let make_fc = |i: usize, ct: ChangeType| FileChange {
            path: PathBuf::from(format!("file_{i}.txt")),
            change_type: ct,
            additions: if ct == ChangeType::Added {
                additions / added.max(1)
            } else {
                0
            },
            deletions: 0,
            is_binary: false,
        };

        let files_added: Vec<FileChange> =
            (0..added).map(|i| make_fc(i, ChangeType::Added)).collect();
        let total_add: usize = files_added.iter().map(|f| f.additions).sum();

        WorkspaceDiff {
            files_added,
            files_modified: (0..modified)
                .map(|i| make_fc(100 + i, ChangeType::Modified))
                .collect(),
            files_deleted: (0..deleted)
                .map(|i| make_fc(200 + i, ChangeType::Deleted))
                .collect(),
            total_additions: total_add.max(additions),
            total_deletions: 0,
        }
    }

    #[test]
    fn policy_max_files_exceeded() {
        let diff = make_diff(5, 3, 2, 10);
        let policy = DiffPolicy {
            max_files: Some(5),
            ..Default::default()
        };
        let result = policy.check(&diff).unwrap();
        assert!(
            matches!(result, PolicyResult::Fail { .. }),
            "10 files should exceed max_files=5"
        );
    }

    #[test]
    fn policy_max_additions_exceeded() {
        let diff = make_diff(1, 0, 0, 1000);
        let policy = DiffPolicy {
            max_additions: Some(100),
            ..Default::default()
        };
        let result = policy.check(&diff).unwrap();
        assert!(
            matches!(result, PolicyResult::Fail { .. }),
            "1000 additions should exceed max_additions=100"
        );
    }

    #[test]
    fn policy_denied_paths_trigger_fail() {
        use abp_workspace::diff::FileChange;
        let diff = WorkspaceDiff {
            files_modified: vec![FileChange {
                path: PathBuf::from("secrets/api_key.env"),
                change_type: ChangeType::Modified,
                additions: 1,
                deletions: 0,
                is_binary: false,
            }],
            total_additions: 1,
            ..Default::default()
        };
        let policy = DiffPolicy {
            denied_paths: vec!["secrets/**".into()],
            ..Default::default()
        };
        let result = policy.check(&diff).unwrap();
        assert!(
            matches!(result, PolicyResult::Fail { .. }),
            "changes in secrets/ should be denied"
        );
    }

    #[test]
    fn policy_passes_when_within_limits() {
        let diff = make_diff(2, 1, 0, 50);
        let policy = DiffPolicy {
            max_files: Some(10),
            max_additions: Some(100),
            denied_paths: vec!["secrets/**".into()],
        };
        let result = policy.check(&diff).unwrap();
        assert!(result.is_pass(), "diff within limits should pass");
    }

    #[test]
    fn empty_policy_always_passes() {
        let diff = make_diff(100, 50, 25, 100_000);
        let policy = DiffPolicy::default();
        let result = policy.check(&diff).unwrap();
        assert!(result.is_pass(), "empty policy should always pass");
    }

    #[test]
    fn policy_with_multiple_constraints() {
        let diff = make_diff(20, 0, 0, 5000);
        let policy = DiffPolicy {
            max_files: Some(10),
            max_additions: Some(100),
            ..Default::default()
        };
        let result = policy.check(&diff).unwrap();
        match result {
            PolicyResult::Fail { violations } => {
                assert!(
                    violations.len() >= 2,
                    "should have at least 2 violations, got: {violations:?}"
                );
            }
            PolicyResult::Pass => panic!("should fail with multiple constraint violations"),
        }
    }

    #[test]
    fn policy_max_files_boundary_pass() {
        let diff = make_diff(3, 1, 1, 10);
        let policy = DiffPolicy {
            max_files: Some(5),
            ..Default::default()
        };
        let result = policy.check(&diff).unwrap();
        assert!(result.is_pass(), "exactly at limit should pass");
    }

    #[test]
    fn policy_denied_paths_no_match_passes() {
        let diff = make_diff(2, 0, 0, 10);
        let policy = DiffPolicy {
            denied_paths: vec!["secrets/**".into()],
            ..Default::default()
        };
        let result = policy.check(&diff).unwrap();
        assert!(
            result.is_pass(),
            "file_0.txt and file_1.txt should not match secrets/**"
        );
    }
}

// ===========================================================================
// Module: end_to_end
// ===========================================================================
mod end_to_end {
    use super::*;

    #[test]
    fn stage_modify_diff_verify() {
        let src = source_dir(&[
            ("src/main.rs", "fn main() {}\n"),
            ("README.md", "# Project\n"),
        ]);
        let ws = stage_default(src.path());

        // Modify a file and add a new one.
        fs::write(
            ws.path().join("src/main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        fs::write(ws.path().join("src/helper.rs"), "pub fn help() {}\n").unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();

        assert_eq!(diff.files_added.len(), 1, "one file added");
        assert_eq!(diff.files_modified.len(), 1, "one file modified");
        assert_eq!(diff.files_deleted.len(), 0, "no files deleted");
        assert!(diff.total_additions > 0, "should have additions");

        // Verify the added file path.
        let added_paths: Vec<_> = diff.files_added.iter().map(|f| &f.path).collect();
        assert!(added_paths.contains(&&PathBuf::from("src/helper.rs")));
    }

    #[test]
    fn stage_policy_check_pass() {
        let src = source_dir(&[("app.rs", "fn app() {}\n")]);
        let ws = stage_default(src.path());

        fs::write(ws.path().join("new.rs"), "fn new() {}\n").unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();

        let policy = DiffPolicy {
            max_files: Some(10),
            max_additions: Some(1000),
            denied_paths: vec!["secrets/**".into()],
        };
        let result = policy.check(&diff).unwrap();
        assert!(result.is_pass());
    }

    #[test]
    fn stage_modify_too_much_policy_fail() {
        let src = source_dir(&[("base.txt", "base\n")]);
        let ws = stage_default(src.path());

        // Create many files to exceed the policy limit.
        for i in 0..20 {
            fs::write(
                ws.path().join(format!("generated_{i}.txt")),
                format!("content {i}\n"),
            )
            .unwrap();
        }

        let analyzer = DiffAnalyzer::new(ws.path());
        let diff = analyzer.analyze().unwrap();

        let policy = DiffPolicy {
            max_files: Some(5),
            ..Default::default()
        };
        let result = policy.check(&diff).unwrap();
        assert!(
            matches!(result, PolicyResult::Fail { .. }),
            "20 new files should exceed max_files=5"
        );
    }

    #[test]
    fn diff_analyzer_has_changes() {
        let src = source_dir(&[("file.txt", "content")]);
        let ws = stage_default(src.path());

        let analyzer = DiffAnalyzer::new(ws.path());
        assert!(!analyzer.has_changes(), "no changes yet");

        fs::write(ws.path().join("file.txt"), "new content").unwrap();
        assert!(analyzer.has_changes(), "should detect changes now");
    }

    #[test]
    fn diff_analyzer_changed_files() {
        let src = source_dir(&[("a.txt", "a"), ("b.txt", "b")]);
        let ws = stage_default(src.path());

        fs::write(ws.path().join("a.txt"), "a modified").unwrap();
        fs::write(ws.path().join("c.txt"), "new file").unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        let changed = analyzer.changed_files();
        assert!(changed.contains(&PathBuf::from("a.txt")));
        assert!(changed.contains(&PathBuf::from("c.txt")));
        assert!(!changed.contains(&PathBuf::from("b.txt")));
    }

    #[test]
    fn diff_analyzer_file_was_modified() {
        let src = source_dir(&[("target.txt", "original")]);
        let ws = stage_default(src.path());

        fs::write(ws.path().join("target.txt"), "changed").unwrap();

        let analyzer = DiffAnalyzer::new(ws.path());
        assert!(analyzer.file_was_modified(Path::new("target.txt")));
        assert!(!analyzer.file_was_modified(Path::new("nonexistent.txt")));
    }

    #[test]
    fn diff_summary_function_on_prepared_workspace() {
        use abp_workspace::diff::diff_workspace;

        let src = source_dir(&[("readme.md", "# Hello\n")]);
        let ws = stage_default(src.path());

        // No changes yet.
        let summary = diff_workspace(&ws).unwrap();
        assert!(summary.is_empty());

        // Make a change.
        fs::write(ws.path().join("new.txt"), "hello world\n").unwrap();
        let summary = diff_workspace(&ws).unwrap();
        assert!(!summary.is_empty());
        assert_eq!(summary.added.len(), 1);
        assert!(summary.total_additions > 0);
    }
}

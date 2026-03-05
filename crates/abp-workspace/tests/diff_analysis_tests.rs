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
//! Tests for diff analysis utilities: `WorkspaceDiff`, `DiffAnalyzer`, `DiffPolicy`.

use abp_workspace::diff::{
    ChangeType, DiffAnalyzer, DiffPolicy, FileChange, PolicyResult, WorkspaceDiff,
};
use abp_workspace::WorkspaceStager;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Stage a workspace from `src` with git init, returning the prepared workspace.
fn staged(src: &Path) -> abp_workspace::PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .stage()
        .expect("staging should succeed")
}

// ===========================================================================
// WorkspaceDiff unit tests
// ===========================================================================

// 1. Default WorkspaceDiff is empty
#[test]
fn workspace_diff_default_is_empty() {
    let diff = WorkspaceDiff::default();
    assert!(diff.is_empty());
    assert_eq!(diff.file_count(), 0);
    assert_eq!(diff.total_additions, 0);
    assert_eq!(diff.total_deletions, 0);
}

// 2. summary() on empty diff
#[test]
fn workspace_diff_summary_no_changes() {
    let diff = WorkspaceDiff::default();
    assert_eq!(diff.summary(), "No changes detected.");
}

// 3. summary() with mixed changes
#[test]
fn workspace_diff_summary_with_changes() {
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("new.txt"),
            change_type: ChangeType::Added,
            additions: 10,
            deletions: 0,
            is_binary: false,
        }],
        files_modified: vec![FileChange {
            path: PathBuf::from("mod.txt"),
            change_type: ChangeType::Modified,
            additions: 5,
            deletions: 3,
            is_binary: false,
        }],
        files_deleted: vec![],
        total_additions: 15,
        total_deletions: 3,
    };
    let s = diff.summary();
    assert!(s.contains("2 file(s) changed"));
    assert!(s.contains("1 added"));
    assert!(s.contains("1 modified"));
    assert!(s.contains("0 deleted"));
    assert!(s.contains("+15"));
    assert!(s.contains("-3"));
}

// 4. file_count reflects all categories
#[test]
fn workspace_diff_file_count() {
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("a"),
            change_type: ChangeType::Added,
            additions: 0,
            deletions: 0,
            is_binary: false,
        }],
        files_modified: vec![
            FileChange {
                path: PathBuf::from("b"),
                change_type: ChangeType::Modified,
                additions: 0,
                deletions: 0,
                is_binary: false,
            },
            FileChange {
                path: PathBuf::from("c"),
                change_type: ChangeType::Modified,
                additions: 0,
                deletions: 0,
                is_binary: false,
            },
        ],
        files_deleted: vec![FileChange {
            path: PathBuf::from("d"),
            change_type: ChangeType::Deleted,
            additions: 0,
            deletions: 0,
            is_binary: false,
        }],
        total_additions: 0,
        total_deletions: 0,
    };
    assert_eq!(diff.file_count(), 4);
    assert!(!diff.is_empty());
}

// 5. WorkspaceDiff serde roundtrip
#[test]
fn workspace_diff_serde_roundtrip() {
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("new.rs"),
            change_type: ChangeType::Added,
            additions: 42,
            deletions: 0,
            is_binary: false,
        }],
        files_modified: vec![],
        files_deleted: vec![FileChange {
            path: PathBuf::from("old.log"),
            change_type: ChangeType::Deleted,
            additions: 0,
            deletions: 10,
            is_binary: false,
        }],
        total_additions: 42,
        total_deletions: 10,
    };
    let json = serde_json::to_string(&diff).unwrap();
    let rt: WorkspaceDiff = serde_json::from_str(&json).unwrap();
    assert_eq!(diff, rt);
}

// ===========================================================================
// FileChange / ChangeType tests
// ===========================================================================

// 6. ChangeType Display
#[test]
fn change_type_display() {
    assert_eq!(format!("{}", ChangeType::Added), "added");
    assert_eq!(format!("{}", ChangeType::Modified), "modified");
    assert_eq!(format!("{}", ChangeType::Deleted), "deleted");
}

// 7. FileChange serde roundtrip
#[test]
fn file_change_serde_roundtrip() {
    let fc = FileChange {
        path: PathBuf::from("src/lib.rs"),
        change_type: ChangeType::Modified,
        additions: 5,
        deletions: 2,
        is_binary: false,
    };
    let json = serde_json::to_string(&fc).unwrap();
    let rt: FileChange = serde_json::from_str(&json).unwrap();
    assert_eq!(fc, rt);
}

// 8. Binary FileChange serde
#[test]
fn binary_file_change_serde() {
    let fc = FileChange {
        path: PathBuf::from("image.png"),
        change_type: ChangeType::Added,
        additions: 0,
        deletions: 0,
        is_binary: true,
    };
    let json = serde_json::to_string(&fc).unwrap();
    assert!(json.contains("\"is_binary\":true"));
    let rt: FileChange = serde_json::from_str(&json).unwrap();
    assert!(rt.is_binary);
}

// ===========================================================================
// DiffAnalyzer integration tests (require git)
// ===========================================================================

// 9. Analyzer on clean workspace shows no changes
#[test]
fn analyzer_clean_workspace_no_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();
    let ws = staged(src.path());

    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(!analyzer.has_changes());

    let diff = analyzer.analyze().unwrap();
    assert!(diff.is_empty());
}

// 10. Analyzer detects added files
#[test]
fn analyzer_detects_added_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("seed.txt"), "seed").unwrap();
    let ws = staged(src.path());

    fs::write(ws.path().join("new.txt"), "alpha\nbeta\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.has_changes());

    let diff = analyzer.analyze().unwrap();
    assert_eq!(diff.files_added.len(), 1);
    assert_eq!(diff.files_added[0].path, PathBuf::from("new.txt"));
    assert_eq!(diff.files_added[0].change_type, ChangeType::Added);
    assert_eq!(diff.files_added[0].additions, 2);
    assert!(!diff.files_added[0].is_binary);
    assert_eq!(diff.total_additions, 2);
}

// 11. Analyzer detects modified files
#[test]
fn analyzer_detects_modified_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("readme.md"), "old\n").unwrap();
    let ws = staged(src.path());

    fs::write(ws.path().join("readme.md"), "new\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    assert_eq!(diff.files_modified.len(), 1);
    assert_eq!(diff.files_modified[0].change_type, ChangeType::Modified);
    assert!(diff.files_modified[0].additions >= 1);
    assert!(diff.files_modified[0].deletions >= 1);
}

// 12. Analyzer detects deleted files
#[test]
fn analyzer_detects_deleted_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "bye\nbye\n").unwrap();
    let ws = staged(src.path());

    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    assert_eq!(diff.files_deleted.len(), 1);
    assert_eq!(diff.files_deleted[0].change_type, ChangeType::Deleted);
    assert!(diff.files_deleted[0].deletions >= 2);
}

// 13. Analyzer mixed changes
#[test]
fn analyzer_mixed_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("modify.txt"), "old\n").unwrap();
    fs::write(src.path().join("delete.txt"), "remove\n").unwrap();
    fs::write(src.path().join("keep.txt"), "same\n").unwrap();
    let ws = staged(src.path());

    fs::write(ws.path().join("modify.txt"), "new\n").unwrap();
    fs::remove_file(ws.path().join("delete.txt")).unwrap();
    fs::write(ws.path().join("added.txt"), "fresh\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    assert_eq!(diff.files_added.len(), 1);
    assert_eq!(diff.files_modified.len(), 1);
    assert_eq!(diff.files_deleted.len(), 1);
    assert_eq!(diff.file_count(), 3);
}

// 14. changed_files() returns sorted paths
#[test]
fn analyzer_changed_files_sorted() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    let ws = staged(src.path());

    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    fs::write(ws.path().join("c.txt"), "c\n").unwrap();
    fs::write(ws.path().join("b.txt"), "changed\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let files = analyzer.changed_files();

    assert_eq!(files.len(), 3);
    // Should be sorted
    assert!(files.windows(2).all(|w| w[0] <= w[1]));
}

// 15. file_was_modified() positive and negative
#[test]
fn analyzer_file_was_modified() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("touched.txt"), "old").unwrap();
    fs::write(src.path().join("untouched.txt"), "same").unwrap();
    let ws = staged(src.path());

    fs::write(ws.path().join("touched.txt"), "new").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.file_was_modified(Path::new("touched.txt")));
    assert!(!analyzer.file_was_modified(Path::new("untouched.txt")));
    assert!(!analyzer.file_was_modified(Path::new("nonexistent.txt")));
}

// 16. Analyzer handles binary files
#[test]
fn analyzer_binary_file_flagged() {
    let src = tempdir().unwrap();
    let png: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
    fs::write(src.path().join("img.png"), &png).unwrap();
    let ws = staged(src.path());

    let mut bigger = png;
    bigger.extend_from_slice(&[0xFF; 32]);
    fs::write(ws.path().join("img.png"), &bigger).unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    assert_eq!(diff.files_modified.len(), 1);
    assert!(diff.files_modified[0].is_binary);
    assert_eq!(diff.files_modified[0].additions, 0);
    assert_eq!(diff.files_modified[0].deletions, 0);
}

// 17. Analyzer nested directory
#[test]
fn analyzer_nested_directory() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "root\n").unwrap();
    let ws = staged(src.path());

    let nested = ws.path().join("sub").join("deep");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("file.txt"), "nested\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    assert!(diff
        .files_added
        .iter()
        .any(|fc| fc.path == Path::new("sub/deep/file.txt")));
}

// ===========================================================================
// DiffPolicy tests
// ===========================================================================

// 18. Empty policy passes any diff
#[test]
fn policy_empty_passes_everything() {
    let policy = DiffPolicy::default();
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("anything.txt"),
            change_type: ChangeType::Added,
            additions: 999,
            deletions: 0,
            is_binary: false,
        }],
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

// 19. max_files violation
#[test]
fn policy_max_files_violation() {
    let policy = DiffPolicy {
        max_files: Some(2),
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        files_added: vec![
            FileChange {
                path: PathBuf::from("a.txt"),
                change_type: ChangeType::Added,
                additions: 1,
                deletions: 0,
                is_binary: false,
            },
            FileChange {
                path: PathBuf::from("b.txt"),
                change_type: ChangeType::Added,
                additions: 1,
                deletions: 0,
                is_binary: false,
            },
            FileChange {
                path: PathBuf::from("c.txt"),
                change_type: ChangeType::Added,
                additions: 1,
                deletions: 0,
                is_binary: false,
            },
        ],
        total_additions: 3,
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = &result {
        assert!(violations.iter().any(|v| v.contains("too many files")));
    }
}

// 20. max_files passes at limit
#[test]
fn policy_max_files_passes_at_limit() {
    let policy = DiffPolicy {
        max_files: Some(1),
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        files_modified: vec![FileChange {
            path: PathBuf::from("ok.txt"),
            change_type: ChangeType::Modified,
            additions: 1,
            deletions: 1,
            is_binary: false,
        }],
        total_additions: 1,
        total_deletions: 1,
        ..Default::default()
    };
    assert!(policy.check(&diff).unwrap().is_pass());
}

// 21. max_additions violation
#[test]
fn policy_max_additions_violation() {
    let policy = DiffPolicy {
        max_additions: Some(10),
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("big.txt"),
            change_type: ChangeType::Added,
            additions: 50,
            deletions: 0,
            is_binary: false,
        }],
        total_additions: 50,
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = &result {
        assert!(violations.iter().any(|v| v.contains("too many additions")));
    }
}

// 22. denied_paths violation
#[test]
fn policy_denied_paths_violation() {
    let policy = DiffPolicy {
        denied_paths: vec!["*.secret".to_string()],
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("creds.secret"),
            change_type: ChangeType::Added,
            additions: 1,
            deletions: 0,
            is_binary: false,
        }],
        total_additions: 1,
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = &result {
        assert!(violations
            .iter()
            .any(|v| v.contains("denied path") && v.contains("creds.secret")));
    }
}

// 23. denied_paths passes on non-matching paths
#[test]
fn policy_denied_paths_passes_non_matching() {
    let policy = DiffPolicy {
        denied_paths: vec!["*.secret".to_string()],
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("readme.md"),
            change_type: ChangeType::Added,
            additions: 1,
            deletions: 0,
            is_binary: false,
        }],
        total_additions: 1,
        ..Default::default()
    };
    assert!(policy.check(&diff).unwrap().is_pass());
}

// 24. Multiple violations reported together
#[test]
fn policy_multiple_violations() {
    let policy = DiffPolicy {
        max_files: Some(1),
        max_additions: Some(5),
        denied_paths: vec!["*.lock".to_string()],
    };
    let diff = WorkspaceDiff {
        files_added: vec![
            FileChange {
                path: PathBuf::from("a.txt"),
                change_type: ChangeType::Added,
                additions: 10,
                deletions: 0,
                is_binary: false,
            },
            FileChange {
                path: PathBuf::from("Cargo.lock"),
                change_type: ChangeType::Added,
                additions: 100,
                deletions: 0,
                is_binary: false,
            },
        ],
        total_additions: 110,
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = &result {
        assert!(
            violations.len() >= 3,
            "expected 3+ violations, got {violations:?}"
        );
    }
}

// 25. PolicyResult serde roundtrip — pass
#[test]
fn policy_result_serde_pass() {
    let result = PolicyResult::Pass;
    let json = serde_json::to_string(&result).unwrap();
    let rt: PolicyResult = serde_json::from_str(&json).unwrap();
    assert_eq!(result, rt);
    assert!(rt.is_pass());
}

// 26. PolicyResult serde roundtrip — fail
#[test]
fn policy_result_serde_fail() {
    let result = PolicyResult::Fail {
        violations: vec!["too many files".to_string()],
    };
    let json = serde_json::to_string(&result).unwrap();
    let rt: PolicyResult = serde_json::from_str(&json).unwrap();
    assert_eq!(result, rt);
    assert!(!rt.is_pass());
}

// 27. DiffPolicy serde roundtrip
#[test]
fn diff_policy_serde_roundtrip() {
    let policy = DiffPolicy {
        max_files: Some(10),
        max_additions: Some(500),
        denied_paths: vec!["*.secret".to_string(), "node_modules/**".to_string()],
    };
    let json = serde_json::to_string(&policy).unwrap();
    let rt: DiffPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.max_files, Some(10));
    assert_eq!(rt.max_additions, Some(500));
    assert_eq!(rt.denied_paths.len(), 2);
}

// 28. Policy on empty diff always passes
#[test]
fn policy_empty_diff_always_passes() {
    let policy = DiffPolicy {
        max_files: Some(0),
        max_additions: Some(0),
        denied_paths: vec!["**".to_string()],
    };
    let diff = WorkspaceDiff::default();
    assert!(policy.check(&diff).unwrap().is_pass());
}

// 29. Analyzer + Policy integration
#[test]
fn analyzer_policy_integration() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("seed.txt"), "seed").unwrap();
    let ws = staged(src.path());

    // Make changes that violate a strict policy
    fs::write(ws.path().join("new1.txt"), "a\n").unwrap();
    fs::write(ws.path().join("new2.txt"), "b\n").unwrap();
    fs::write(ws.path().join("new3.txt"), "c\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    let strict = DiffPolicy {
        max_files: Some(1),
        ..Default::default()
    };
    assert!(!strict.check(&diff).unwrap().is_pass());

    let lenient = DiffPolicy {
        max_files: Some(10),
        ..Default::default()
    };
    assert!(lenient.check(&diff).unwrap().is_pass());
}

// 30. summary() wording for deletions only
#[test]
fn workspace_diff_summary_deletions_only() {
    let diff = WorkspaceDiff {
        files_deleted: vec![FileChange {
            path: PathBuf::from("removed.txt"),
            change_type: ChangeType::Deleted,
            additions: 0,
            deletions: 5,
            is_binary: false,
        }],
        total_deletions: 5,
        ..Default::default()
    };
    let s = diff.summary();
    assert!(s.contains("1 file(s) changed"));
    assert!(s.contains("0 added"));
    assert!(s.contains("1 deleted"));
    assert!(s.contains("-5"));
}

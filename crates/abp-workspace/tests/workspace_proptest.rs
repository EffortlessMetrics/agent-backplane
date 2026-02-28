// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deeper property-based tests for `abp-workspace`.

use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use proptest::prelude::*;
use std::fs;
use std::process::Command;

// ── Strategies ──────────────────────────────────────────────────────

/// Safe path segment: lowercase alphanumeric with optional underscore.
fn path_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_map(|s| s.to_string())
}

/// A (relative_path, content) pair for building arbitrary file trees.
fn arb_file_entry() -> impl Strategy<Value = (String, Vec<u8>)> {
    (
        prop::collection::vec(path_segment(), 1..=3).prop_map(|segs| segs.join("/")),
        prop::collection::vec(any::<u8>(), 0..128),
    )
}

/// Safe glob pattern (subset that is always syntactically valid).
fn safe_glob_pattern() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z][a-z0-9]{0,5}".prop_map(|s| s),
        "[a-z]{1,4}".prop_map(|ext| format!("*.{ext}")),
        "[a-z]{1,4}".prop_map(|ext| format!("**/*.{ext}")),
        "[a-z][a-z0-9]{0,3}".prop_map(|dir| format!("{dir}/**")),
    ]
}

/// Path components containing special characters that should not cause panics.
fn special_segment() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z][a-z0-9 ]{0,7}".prop_map(|s| s),
        "[a-z][a-z0-9._-]{0,7}".prop_map(|s| s),
        "[a-z][a-z0-9@#]{0,5}".prop_map(|s| s),
    ]
}

// ── 1. Arbitrary file trees → staging preserves content integrity ───

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn staging_preserves_content_integrity(
        entries in prop::collection::vec(arb_file_entry(), 1..6),
    ) {
        let src = tempfile::tempdir().unwrap();

        // Write arbitrary files into the source tree.
        for (rel_path, content) in &entries {
            let full = src.path().join(rel_path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, content).unwrap();
        }

        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();

        // Every file should have identical content in the staged workspace.
        for (rel_path, content) in &entries {
            let staged = ws.path().join(rel_path);
            let staged_content = fs::read(&staged).unwrap();
            prop_assert_eq!(
                &staged_content, content,
                "content mismatch for {}", rel_path
            );
        }
    }
}

// ── 2. Random include/exclude patterns → decision is always Allowed or Denied

proptest! {
    #[test]
    fn include_exclude_decision_is_always_valid(
        path in path_segment(),
        includes in prop::collection::vec(safe_glob_pattern(), 0..3),
        excludes in prop::collection::vec(safe_glob_pattern(), 0..3),
    ) {
        let globs = IncludeExcludeGlobs::new(&includes, &excludes);
        if let Ok(g) = globs {
            let decision = g.decide_str(&path);
            prop_assert!(matches!(
                decision,
                MatchDecision::Allowed
                    | MatchDecision::DeniedByExclude
                    | MatchDecision::DeniedByMissingInclude
            ));
        }
    }
}

// ── 3. Path components with special chars → no panics during staging

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn special_char_paths_do_not_panic(
        segments in prop::collection::vec(special_segment(), 1..=3),
        content in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let src = tempfile::tempdir().unwrap();

        let rel = segments.join("/");
        let full = src.path().join(&rel);
        if let Some(parent) = full.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&full, &content);

        // Should not panic regardless of path content.
        let _result = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage();
    }
}

// ── 4. Git init → always produces exactly 1 commit ─────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn git_init_produces_exactly_one_commit(
        entries in prop::collection::vec(arb_file_entry(), 1..4),
    ) {
        let src = tempfile::tempdir().unwrap();
        for (rel_path, content) in &entries {
            let full = src.path().join(rel_path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, content).unwrap();
        }

        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(true)
            .stage()
            .unwrap();

        // Count commits in the initialized repo.
        let output = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(ws.path())
            .output()
            .unwrap();

        let count_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        prop_assert_eq!(count_str, "1", "expected exactly 1 commit");
    }
}

// ── 5. Exclude patterns actually exclude files ──────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn exclude_patterns_filter_files(
        seg in path_segment(),
    ) {
        let src = tempfile::tempdir().unwrap();

        // Create a file that will match *.log
        let log_file = format!("{seg}.log");
        fs::write(src.path().join(&log_file), "data").unwrap();

        // Create a file that will NOT match *.log
        let txt_file = format!("{seg}.txt");
        fs::write(src.path().join(&txt_file), "data").unwrap();

        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .exclude(vec!["*.log".into()])
            .with_git_init(false)
            .stage()
            .unwrap();

        prop_assert!(!ws.path().join(&log_file).exists(), "log file should be excluded");
        prop_assert!(ws.path().join(&txt_file).exists(), "txt file should be included");
    }
}

// ── 6. Decision determinism ─────────────────────────────────────────

proptest! {
    #[test]
    fn include_exclude_decision_is_deterministic(
        path in path_segment(),
        includes in prop::collection::vec(safe_glob_pattern(), 0..3),
        excludes in prop::collection::vec(safe_glob_pattern(), 0..3),
    ) {
        if let Ok(g) = IncludeExcludeGlobs::new(&includes, &excludes) {
            let d1 = g.decide_str(&path);
            let d2 = g.decide_str(&path);
            prop_assert_eq!(d1, d2, "decision must be deterministic");
        }
    }
}

// ── 7. git_status/git_diff return Some after staging ────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn git_status_and_diff_return_some_after_staging(
        content in "[a-z]{1,32}",
    ) {
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("file.txt"), content.as_bytes()).unwrap();

        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(true)
            .stage()
            .unwrap();

        // After staging with git init, git_status should succeed.
        let status = WorkspaceManager::git_status(ws.path());
        prop_assert!(status.is_some(), "git_status should return Some");

        let diff = WorkspaceManager::git_diff(ws.path());
        prop_assert!(diff.is_some(), "git_diff should return Some");
    }
}

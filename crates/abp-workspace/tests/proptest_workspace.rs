// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `abp-workspace`.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::WorkspaceManager;
use proptest::prelude::*;
use std::fs;

// ── Arbitrary strategies ────────────────────────────────────────────

fn arb_workspace_mode() -> impl Strategy<Value = WorkspaceMode> {
    prop_oneof![
        Just(WorkspaceMode::PassThrough),
        Just(WorkspaceMode::Staged),
    ]
}

fn arb_workspace_spec() -> impl Strategy<Value = WorkspaceSpec> {
    (".*", arb_workspace_mode()).prop_map(|(root, mode)| WorkspaceSpec {
        root,
        mode,
        include: vec![],
        exclude: vec![],
    })
}

/// Safe alphanumeric glob pattern (e.g. `src`, `*.rs`, `**/*.txt`).
fn safe_glob_pattern() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z][a-z0-9]{0,5}".prop_map(|s| s),
        "[a-z]{1,4}".prop_map(|ext| format!("*.{ext}")),
        "[a-z]{1,4}".prop_map(|ext| format!("**/*.{ext}")),
        "[a-z][a-z0-9]{0,3}".prop_map(|dir| format!("{dir}/**")),
    ]
}

// ── 1. WorkspaceSpec serde round-trip ───────────────────────────────

proptest! {
    #[test]
    fn workspace_spec_serde_round_trip(spec in arb_workspace_spec()) {
        let json = serde_json::to_string(&spec).unwrap();
        let deser: WorkspaceSpec = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deser).unwrap();
        prop_assert_eq!(json, json2);
    }
}

// ── 2. WorkspaceMode serde round-trip ───────────────────────────────

proptest! {
    #[test]
    fn workspace_mode_serde_round_trip(mode in arb_workspace_mode()) {
        let json = serde_json::to_string(&mode).unwrap();
        let deser: WorkspaceMode = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deser).unwrap();
        prop_assert_eq!(json, json2);
    }
}

// ── 3. Alphanumeric glob patterns never panic the staging function ──

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn staging_with_safe_globs_never_panics(
        includes in prop::collection::vec(safe_glob_pattern(), 0..3),
        excludes in prop::collection::vec(safe_glob_pattern(), 0..3),
    ) {
        // Create a real temporary source directory with a file so staging
        // has something to walk.
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("hello.txt"), "world").unwrap();

        let spec = WorkspaceSpec {
            root: src.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::Staged,
            include: includes,
            exclude: excludes,
        };

        // Must not panic — errors are acceptable (invalid globs), panics are not.
        let _result = WorkspaceManager::prepare(&spec);
    }
}

// ── 4. WorkspaceSpec with random include/exclude is always constructable ─

proptest! {
    #[test]
    fn workspace_spec_always_constructable(
        root in ".*",
        mode in arb_workspace_mode(),
        includes in prop::collection::vec(safe_glob_pattern(), 0..5),
        excludes in prop::collection::vec(safe_glob_pattern(), 0..5),
    ) {
        let spec = WorkspaceSpec {
            root,
            mode,
            include: includes,
            exclude: excludes,
        };

        // Construction always succeeds and serde round-trips.
        let json = serde_json::to_string(&spec).unwrap();
        let deser: WorkspaceSpec = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(spec.root, deser.root);
        prop_assert_eq!(spec.include, deser.include);
        prop_assert_eq!(spec.exclude, deser.exclude);
    }
}

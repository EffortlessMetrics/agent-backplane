// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for enhanced workspace staging: metadata, validation, cleanup,
//! snapshots, diff extraction, and content hashing.

use abp_workspace::{workspace_content_hash, WorkspaceStager};
use std::fs;
use tempfile::tempdir;

// ── Helpers ─────────────────────────────────────────────────────────────

/// Create a small source tree for testing.
fn make_source_tree() -> tempfile::TempDir {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "hello world").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("nested.txt"), "nested content").unwrap();
    src
}

fn stage_from(src: &std::path::Path) -> abp_workspace::PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .stage()
        .expect("staging should succeed")
}

// ── Metadata tests ──────────────────────────────────────────────────────

#[test]
fn metadata_reports_file_and_dir_counts() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let meta = ws.metadata().expect("metadata should succeed");
    assert_eq!(meta.file_count, 2, "expected 2 files");
    assert!(meta.dir_count >= 1, "expected at least 1 sub-directory");
    assert!(meta.total_size > 0, "total size should be > 0");
}

#[test]
fn metadata_total_size_matches_file_contents() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let meta = ws.metadata().unwrap();
    let expected_size = "hello world".len() as u64 + "nested content".len() as u64;
    assert_eq!(meta.total_size, expected_size);
}

#[test]
fn created_at_is_set() {
    let src = make_source_tree();
    let ws = stage_from(src.path());
    // created_at should be recent (within the last 60 seconds).
    let elapsed = chrono::Utc::now() - ws.created_at();
    assert!(elapsed.num_seconds() < 60);
}

#[test]
fn is_staged_returns_true_for_staged_workspaces() {
    let src = make_source_tree();
    let ws = stage_from(src.path());
    assert!(ws.is_staged());
}

// ── Validation tests ────────────────────────────────────────────────────

#[test]
fn validate_passes_for_healthy_staged_workspace() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let result = ws.validate();
    assert!(result.is_valid(), "problems: {:?}", result.problems);
}

#[test]
fn validate_detects_missing_git_dir() {
    let src = make_source_tree();
    // Stage without git init.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let result = ws.validate();
    assert!(!result.is_valid());
    assert!(
        result.problems.iter().any(|p| p.contains(".git")),
        "expected .git complaint, got: {:?}",
        result.problems
    );
}

// ── Snapshot tests ──────────────────────────────────────────────────────

#[test]
fn snapshot_captures_workspace_state() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let snap = ws.snapshot().expect("snapshot should succeed");
    assert_eq!(snap.file_count(), 2);
    assert!(snap.total_size() > 0);
}

#[test]
fn snapshot_detects_modifications() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let snap_before = ws.snapshot().unwrap();

    // Modify a file in the staged workspace.
    fs::write(ws.path().join("hello.txt"), "modified content").unwrap();

    let snap_after = ws.snapshot().unwrap();

    let diff = abp_workspace::snapshot::compare(&snap_before, &snap_after);
    assert!(!diff.modified.is_empty(), "should detect modified file");
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
}

#[test]
fn snapshot_detects_added_and_removed_files() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let snap_before = ws.snapshot().unwrap();

    // Add a new file.
    fs::write(ws.path().join("new_file.txt"), "brand new").unwrap();
    // Remove an existing file.
    fs::remove_file(ws.path().join("hello.txt")).unwrap();

    let snap_after = ws.snapshot().unwrap();
    let diff = abp_workspace::snapshot::compare(&snap_before, &snap_after);

    assert!(
        diff.added.iter().any(|p| p.ends_with("new_file.txt")),
        "should detect added file"
    );
    assert!(
        diff.removed.iter().any(|p| p.ends_with("hello.txt")),
        "should detect removed file"
    );
}

// ── Diff extraction tests ───────────────────────────────────────────────

#[test]
fn diff_summary_on_clean_workspace_is_empty() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let diff = ws.diff_summary().expect("diff_summary should succeed");
    assert!(diff.is_empty(), "no changes expected on fresh workspace");
}

#[test]
fn diff_summary_detects_new_file() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    fs::write(ws.path().join("added.txt"), "new content").unwrap();

    let diff = ws.diff_summary().unwrap();
    assert!(
        diff.added.iter().any(|p| p.ends_with("added.txt")),
        "should detect added file in diff_summary"
    );
}

#[test]
fn changed_files_classifies_modifications() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    fs::write(ws.path().join("hello.txt"), "updated content").unwrap();

    let changes = ws.changed_files().unwrap();
    assert!(
        !changes.is_empty(),
        "should detect at least one changed file"
    );
    assert!(changes
        .iter()
        .any(|fc| fc.change_type == abp_workspace::diff::ChangeType::Modified));
}

// ── Cleanup tests ───────────────────────────────────────────────────────

#[test]
fn cleanup_removes_staged_directory() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let ws_path = ws.path().to_path_buf();
    assert!(ws_path.exists(), "workspace should exist before cleanup");

    ws.cleanup().expect("cleanup should succeed");
    assert!(
        !ws_path.exists(),
        "workspace directory should be removed after cleanup"
    );
}

// ── Content hash tests ──────────────────────────────────────────────────

#[test]
fn content_hash_is_deterministic() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let h1 = workspace_content_hash(ws.path()).unwrap();
    let h2 = workspace_content_hash(ws.path()).unwrap();
    assert_eq!(h1, h2, "same workspace should produce same hash");
}

#[test]
fn content_hash_changes_on_modification() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let h_before = workspace_content_hash(ws.path()).unwrap();
    fs::write(ws.path().join("hello.txt"), "changed!").unwrap();
    let h_after = workspace_content_hash(ws.path()).unwrap();

    assert_ne!(h_before, h_after, "hash should change after modification");
}

#[test]
fn content_hash_changes_on_file_addition() {
    let src = make_source_tree();
    let ws = stage_from(src.path());

    let h_before = workspace_content_hash(ws.path()).unwrap();
    fs::write(ws.path().join("extra.txt"), "extra").unwrap();
    let h_after = workspace_content_hash(ws.path()).unwrap();

    assert_ne!(h_before, h_after, "hash should change after adding a file");
}

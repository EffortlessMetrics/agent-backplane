// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive git diff capture tests for staged workspaces.
//!
//! These tests verify that after `WorkspaceManager` stages a workspace (with a
//! baseline git commit), subsequent file mutations are correctly reflected by
//! `git diff` and `git status`.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::WorkspaceManager;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `Staged` workspace spec from a source path.
fn staged_spec(root: &str) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

/// Run an arbitrary git command inside `dir` and return trimmed stdout.
fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ---------------------------------------------------------------------------
// 1. Fresh workspace has no diff
// ---------------------------------------------------------------------------
#[test]
fn fresh_workspace_has_no_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty(), "expected clean status, got: {status}");

    let diff = git(ws.path(), &["diff", "--no-color"]);
    assert!(diff.is_empty(), "expected empty diff, got: {diff}");
}

// ---------------------------------------------------------------------------
// 2. Modified file produces diff
// ---------------------------------------------------------------------------
#[test]
fn modified_file_produces_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("greeting.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    // Mutate after baseline commit
    fs::write(ws.path().join("greeting.txt"), "goodbye").unwrap();

    let diff = git(ws.path(), &["diff", "--no-color"]);
    assert!(
        diff.contains("greeting.txt"),
        "diff should reference changed file"
    );
    assert!(diff.contains("-hello"), "diff should show removed line");
    assert!(diff.contains("+goodbye"), "diff should show added line");
}

// ---------------------------------------------------------------------------
// 3. New file produces diff
// ---------------------------------------------------------------------------
#[test]
fn new_file_produces_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("original.txt"), "seed").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    fs::write(ws.path().join("brand_new.txt"), "I am new").unwrap();

    // Untracked files don't appear in `git diff` — use `git diff` after staging.
    git(ws.path(), &["add", "brand_new.txt"]);
    let diff = git(ws.path(), &["diff", "--cached", "--no-color"]);
    assert!(
        diff.contains("brand_new.txt"),
        "diff should reference new file"
    );
    assert!(
        diff.contains("+I am new"),
        "diff should contain new file content"
    );

    // Also verify porcelain status shows the addition.
    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(
        status.contains("brand_new.txt"),
        "status should list new file"
    );
}

// ---------------------------------------------------------------------------
// 4. Deleted file produces diff
// ---------------------------------------------------------------------------
#[test]
fn deleted_file_produces_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "will be removed").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let diff = git(ws.path(), &["diff", "--no-color"]);
    assert!(
        diff.contains("doomed.txt"),
        "diff should reference deleted file"
    );
    assert!(
        diff.contains("-will be removed"),
        "diff should show removed content"
    );

    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(
        status.contains("D doomed.txt"),
        "status should mark deletion, got: {status}"
    );
}

// ---------------------------------------------------------------------------
// 5. Multiple changes produce combined diff
// ---------------------------------------------------------------------------
#[test]
fn multiple_changes_produce_combined_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("modify_me.txt"), "original").unwrap();
    fs::write(src.path().join("delete_me.txt"), "bye").unwrap();
    fs::write(src.path().join("keep.txt"), "unchanged").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    // Modify
    fs::write(ws.path().join("modify_me.txt"), "changed").unwrap();
    // Delete
    fs::remove_file(ws.path().join("delete_me.txt")).unwrap();
    // Add
    fs::write(ws.path().join("added.txt"), "new content").unwrap();

    // Stage everything so all changes appear in a single diff.
    git(ws.path(), &["add", "-A"]);
    let diff = git(ws.path(), &["diff", "--cached", "--no-color"]);

    assert!(
        diff.contains("modify_me.txt"),
        "diff should contain modified file"
    );
    assert!(
        diff.contains("delete_me.txt"),
        "diff should contain deleted file"
    );
    assert!(diff.contains("added.txt"), "diff should contain added file");
    // Unchanged file must NOT appear.
    assert!(
        !diff.contains("keep.txt"),
        "diff should not contain unchanged file"
    );
}

// ---------------------------------------------------------------------------
// 6. Binary files are handled
// ---------------------------------------------------------------------------
#[test]
fn binary_files_are_handled() {
    let src = tempdir().unwrap();
    // Write a small PNG header (binary content).
    let png_header: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    ];
    fs::write(src.path().join("image.png"), &png_header).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    // Modify binary file
    let mut modified = png_header.clone();
    modified.extend_from_slice(&[0xFF; 16]);
    fs::write(ws.path().join("image.png"), &modified).unwrap();

    // Should not panic — git reports "Binary files differ"
    let diff = git(ws.path(), &["diff", "--no-color"]);
    assert!(
        diff.contains("image.png"),
        "diff should reference binary file"
    );
    assert!(
        diff.contains("Binary files") || diff.contains("GIT binary patch") || diff.contains("Bin"),
        "diff should indicate binary change, got: {diff}"
    );

    // Also add a new binary file
    fs::write(ws.path().join("new.bin"), vec![0u8; 64]).unwrap();
    git(ws.path(), &["add", "new.bin"]);
    let cached = git(ws.path(), &["diff", "--cached", "--no-color"]);
    assert!(
        cached.contains("new.bin"),
        "diff should reference new binary file"
    );
}

// ---------------------------------------------------------------------------
// 7. Large file changes
// ---------------------------------------------------------------------------
#[test]
fn large_file_changes() {
    let src = tempdir().unwrap();
    // Create a ~100 KB file (many lines).
    let original: String = (0..2000).map(|i| format!("line {i}\n")).collect();
    fs::write(src.path().join("big.txt"), &original).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    // Replace every line
    let replacement: String = (0..2000).map(|i| format!("REPLACED {i}\n")).collect();
    fs::write(ws.path().join("big.txt"), &replacement).unwrap();

    let diff = git(ws.path(), &["diff", "--no-color", "--stat"]);
    assert!(
        diff.contains("big.txt"),
        "diff --stat should list big.txt, got: {diff}"
    );

    // Full diff should still work without error.
    let full = git(ws.path(), &["diff", "--no-color"]);
    assert!(
        full.contains("+REPLACED 0"),
        "full diff should contain replacement content"
    );
    assert!(
        full.contains("-line 0"),
        "full diff should contain removed content"
    );
}

// ---------------------------------------------------------------------------
// 8. Nested directory changes
// ---------------------------------------------------------------------------
#[test]
fn nested_directory_changes() {
    let src = tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.txt"), "deep content").unwrap();
    fs::write(src.path().join("root.txt"), "root content").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    // Modify nested file
    fs::write(
        ws.path().join("a").join("b").join("c").join("deep.txt"),
        "modified deep",
    )
    .unwrap();
    // Add new nested file
    let new_dir = ws.path().join("x").join("y");
    fs::create_dir_all(&new_dir).unwrap();
    fs::write(new_dir.join("new_nested.txt"), "hello from nested").unwrap();
    // Delete root-level file
    fs::remove_file(ws.path().join("root.txt")).unwrap();

    // Stage and diff
    git(ws.path(), &["add", "-A"]);
    let diff = git(ws.path(), &["diff", "--cached", "--no-color"]);

    assert!(
        diff.contains("a/b/c/deep.txt"),
        "diff should show nested modified file"
    );
    assert!(
        diff.contains("x/y/new_nested.txt"),
        "diff should show newly added nested file"
    );
    assert!(
        diff.contains("root.txt"),
        "diff should show deleted root file"
    );
    assert!(
        diff.contains("+modified deep"),
        "diff should contain updated nested content"
    );
    assert!(
        diff.contains("+hello from nested"),
        "diff should contain new nested content"
    );
}

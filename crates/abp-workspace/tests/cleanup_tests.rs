// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for workspace cleanup behavior and error paths.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::WorkspaceManager;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn staged_spec(root: &str) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

// ── Cleanup / lifetime tests ────────────────────────────────────────

#[test]
fn staged_temp_dir_exists_during_use() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "data").unwrap();

    let prepared = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();
    let ws_path = prepared.path().to_path_buf();

    assert!(
        ws_path.exists(),
        "temp dir must exist while PreparedWorkspace is alive"
    );
    assert!(ws_path.join("a.txt").exists());
}

#[test]
fn staged_temp_dir_cleaned_up_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("b.txt"), "data").unwrap();

    let ws_path: PathBuf;
    {
        let prepared =
            WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();
        ws_path = prepared.path().to_path_buf();
        assert!(ws_path.exists(), "temp dir must exist before drop");
        // `prepared` is dropped here
    }

    assert!(
        !ws_path.exists(),
        "temp dir must be removed after PreparedWorkspace is dropped"
    );
}

#[test]
fn multiple_staged_workspaces_coexist() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "hello").unwrap();

    let root = src.path().to_string_lossy().to_string();
    let ws1 = WorkspaceManager::prepare(&staged_spec(&root)).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(&root)).unwrap();

    // Different temp directories
    assert_ne!(ws1.path(), ws2.path());

    // Both are independently valid
    assert!(ws1.path().join("shared.txt").exists());
    assert!(ws2.path().join("shared.txt").exists());

    // Dropping one does not affect the other
    let ws2_path = ws2.path().to_path_buf();
    drop(ws1);
    assert!(
        ws2_path.exists(),
        "dropping one workspace must not affect another"
    );
}

// ── Error path tests ────────────────────────────────────────────────

#[test]
fn non_existent_source_root_returns_error() {
    let bogus = PathBuf::from("__this_path_does_not_exist_abp_test__");
    assert!(!bogus.exists());

    let result = WorkspaceManager::prepare(&staged_spec(&bogus.to_string_lossy()));
    assert!(
        result.is_err(),
        "preparing a workspace from a non-existent root should fail"
    );
}

#[test]
fn empty_source_directory_succeeds() {
    let src = tempdir().unwrap();
    // No files — empty directory

    let prepared = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    assert!(prepared.path().exists());
    // ensure_git_repo should still initialise a .git directory
    assert!(
        prepared.path().join(".git").exists(),
        "empty staged workspace should still get a git repo"
    );
}

// ── Default .git exclusion ──────────────────────────────────────────

#[test]
fn staged_excludes_dot_git_by_default() {
    let src = tempdir().unwrap();
    let git_dir = src.path().join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    // Place a unique sentinel file that real git-init would never create
    fs::write(git_dir.join("abp_test_sentinel"), "marker").unwrap();
    fs::write(src.path().join("main.rs"), "fn main() {}").unwrap();

    let prepared = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    assert!(prepared.path().join("main.rs").exists());
    assert!(
        !prepared
            .path()
            .join(".git")
            .join("abp_test_sentinel")
            .exists(),
        "source .git contents must not be copied into staged workspace"
    );
}

// ── Include / exclude glob filtering ────────────────────────────────

#[test]
fn include_globs_filter_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("lib.rs"), "pub fn f() {}").unwrap();
    fs::write(src.path().join("notes.md"), "# Notes").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".to_string()],
        exclude: vec![],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert!(prepared.path().join("lib.rs").exists());
    assert!(
        !prepared.path().join("notes.md").exists(),
        "non-matching files should be excluded by include globs"
    );
}

#[test]
fn exclude_globs_filter_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("app.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("secret.env"), "KEY=val").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["*.env".to_string()],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert!(prepared.path().join("app.rs").exists());
    assert!(
        !prepared.path().join("secret.env").exists(),
        "excluded files should not appear in staged workspace"
    );
}

#[test]
fn combined_include_exclude_globs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "// keep").unwrap();
    fs::write(src.path().join("skip.txt"), "skip").unwrap();
    fs::write(src.path().join("generated.rs"), "// gen").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".to_string()],
        exclude: vec!["generated.*".to_string()],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert!(prepared.path().join("keep.rs").exists());
    assert!(!prepared.path().join("skip.txt").exists());
    assert!(
        !prepared.path().join("generated.rs").exists(),
        "exclude should override include"
    );
}

// ── Git initialisation ──────────────────────────────────────────────

#[test]
fn git_repo_initialised_in_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let prepared = WorkspaceManager::prepare(&staged_spec(&src.path().to_string_lossy())).unwrap();

    assert!(
        prepared.path().join(".git").exists(),
        "staged workspace must have an initialised git repo"
    );

    // Verify we can run git commands against it
    let status = WorkspaceManager::git_status(prepared.path());
    assert!(
        status.is_some(),
        "git_status should succeed on initialised repo"
    );
    // Baseline commit means working tree is clean
    let status = status.unwrap();
    assert!(
        status.trim().is_empty(),
        "fresh staged workspace should have clean git status, got: {status}"
    );
}

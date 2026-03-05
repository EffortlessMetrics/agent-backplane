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
//! Integration tests for workspace staging behavior.
//!
//! Covers: temp dir creation, file copy, glob filtering, git initialization,
//! baseline commit, diff generation, .git exclusion, nested directories,
//! symlinks, empty directories, large files, hidden files, unicode filenames,
//! read-only files, and cleanup.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_glob::IncludeExcludeGlobs;
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::Path;

/// Helper: create a source tree with some files inside a tempdir.
fn make_source_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create source tempdir");
    let root = dir.path();

    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("src").join("lib.rs"), "pub fn lib() {}").unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests").join("test1.rs"), "#[test] fn t() {}").unwrap();

    dir
}

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

// ── 1. Temp dir creation ────────────────────────────────────────────────

#[test]
fn staging_creates_temporary_directory() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .expect("stage workspace");

    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

#[test]
fn staging_via_workspace_manager_creates_temp_dir() {
    let src = make_source_tree();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).expect("prepare staged workspace");
    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

#[test]
fn staging_path_differs_from_source() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_ne!(ws.path(), src.path());
}

// ── 2. File copy ────────────────────────────────────────────────────────

#[test]
fn source_files_are_copied_to_staging() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join("README.md").is_file());
    assert!(ws.path().join("Cargo.toml").is_file());
    assert!(ws.path().join("src").join("main.rs").is_file());
}

#[test]
fn copied_file_contents_match_source() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let original = fs::read_to_string(src.path().join("README.md")).unwrap();
    let staged = fs::read_to_string(ws.path().join("README.md")).unwrap();
    assert_eq!(original, staged);
}

#[test]
fn all_regular_files_copied_without_filters() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join("src").join("lib.rs").is_file());
    assert!(ws.path().join("tests").join("test1.rs").is_file());
}

// ── 3. Glob filtering ──────────────────────────────────────────────────

#[test]
fn include_filter_copies_only_matching_files() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**"]))
        .stage()
        .unwrap();

    assert!(ws.path().join("src").join("main.rs").is_file());
    assert!(!ws.path().join("README.md").exists());
    assert!(!ws.path().join("Cargo.toml").exists());
}

#[test]
fn exclude_filter_skips_matching_files() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["tests/**"]))
        .stage()
        .unwrap();

    assert!(ws.path().join("README.md").is_file());
    assert!(ws.path().join("src").join("main.rs").is_file());
    assert!(!ws.path().join("tests").join("test1.rs").exists());
}

#[test]
fn exclude_overrides_include() {
    let src = make_source_tree();
    // Include src/** but exclude src/lib.rs
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**"]))
        .exclude(patterns(&["src/lib.rs"]))
        .stage()
        .unwrap();

    assert!(ws.path().join("src").join("main.rs").is_file());
    assert!(!ws.path().join("src").join("lib.rs").exists());
}

#[test]
fn combined_include_exclude_via_workspace_spec() {
    let src = make_source_tree();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: patterns(&["src/**", "*.toml"]),
        exclude: patterns(&["src/lib.rs"]),
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("Cargo.toml").is_file());
    assert!(ws.path().join("src").join("main.rs").is_file());
    assert!(!ws.path().join("src").join("lib.rs").exists());
    assert!(!ws.path().join("README.md").exists());
}

#[test]
fn glob_decision_consistency_with_staging() {
    let globs =
        IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["src/lib.rs"])).unwrap();

    assert!(globs.decide_str("src/main.rs").is_allowed());
    assert!(!globs.decide_str("src/lib.rs").is_allowed());
    assert!(!globs.decide_str("README.md").is_allowed());
}

// ── 4. Git initialization ───────────────────────────────────────────────

#[test]
fn staging_initializes_git_repo() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join(".git").exists());
}

#[test]
fn staging_without_git_init_skips_git() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
}

// ── 5. Baseline commit ─────────────────────────────────────────────────

#[test]
fn baseline_commit_exists_after_staging() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(ws.path())
        .output()
        .expect("run git log");

    let log = String::from_utf8_lossy(&output.stdout);
    assert!(
        log.contains("baseline"),
        "expected 'baseline' in git log, got: {log}"
    );
}

#[test]
fn git_status_clean_after_staging() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert_eq!(status.as_deref(), Some(""));
}

// ── 6. Diff generation ─────────────────────────────────────────────────

#[test]
fn diff_empty_when_no_changes_made() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let diff = WorkspaceManager::git_diff(ws.path());
    assert_eq!(diff.as_deref(), Some(""));
}

#[test]
fn diff_shows_modifications_after_edit() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // Modify a tracked file.
    fs::write(ws.path().join("README.md"), "# Modified").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path()).expect("git diff should succeed");
    assert!(
        diff.contains("Modified"),
        "diff should contain new content: {diff}"
    );
}

#[test]
fn git_status_shows_new_untracked_file() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("new_file.txt"), "new content").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).expect("git status should succeed");
    assert!(
        status.contains("new_file.txt"),
        "status should show untracked file: {status}"
    );
}

// ── 7. .git exclusion ───────────────────────────────────────────────────

#[test]
fn git_directory_not_copied_from_source() {
    let src = make_source_tree();

    // Create a fake .git dir in the source.
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // .git must NOT be copied from source.
    assert!(!ws.path().join(".git").join("HEAD").exists());
    // Files should still be present.
    assert!(ws.path().join("README.md").is_file());
}

#[test]
fn fresh_git_repo_replaces_source_git_dir() {
    let src = make_source_tree();

    // Create a .git dir in source with a sentinel file.
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("sentinel"), "source-git").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // The sentinel should NOT exist — old .git was excluded.
    assert!(!ws.path().join(".git").join("sentinel").exists());
    // But a fresh .git should have been initialized.
    assert!(ws.path().join(".git").exists());
}

// ── 8. Nested directories ───────────────────────────────────────────────

#[test]
fn deeply_nested_directory_structure_preserved() {
    let src = tempfile::tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "deep content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged_leaf = ws
        .path()
        .join("a")
        .join("b")
        .join("c")
        .join("d")
        .join("leaf.txt");
    assert!(staged_leaf.is_file());
    assert_eq!(fs::read_to_string(staged_leaf).unwrap(), "deep content");
}

#[test]
fn multiple_sibling_directories_preserved() {
    let src = tempfile::tempdir().unwrap();
    for name in &["alpha", "beta", "gamma"] {
        let dir = src.path().join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), name).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    for name in &["alpha", "beta", "gamma"] {
        let staged = ws.path().join(name).join("file.txt");
        assert!(staged.is_file(), "missing {name}/file.txt");
        assert_eq!(fs::read_to_string(staged).unwrap(), *name);
    }
}

// ── 9. Symbolic links ──────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed() {
    use std::os::unix::fs::symlink;

    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real content").unwrap();
    symlink(src.path().join("real.txt"), src.path().join("link.txt")).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The real file must be copied.
    assert!(ws.path().join("real.txt").is_file());
    // The symlink is not followed (follow_links=false), so it should NOT appear
    // as a regular file in the staging area.
    // (walkdir skips symlinks when follow_links is false and the entry is not a dir/file)
    // The behavior is: symlink entries have file_type().is_symlink() = true,
    // and the copy logic only copies is_file() and is_dir(), so symlinks are skipped.
    assert!(
        !ws.path().join("link.txt").is_file(),
        "symlink should not be copied as a regular file"
    );
}

#[cfg(windows)]
#[test]
fn symlinks_are_skipped_on_windows() {
    // On Windows, symlink creation often requires elevated privileges.
    // We just verify that staging works when no symlinks are present.
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("real.txt").is_file());
}

// ── 10. Empty directories ───────────────────────────────────────────────

#[test]
fn empty_directory_is_created_in_staging() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_dir")).unwrap();
    fs::write(src.path().join("anchor.txt"), "x").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("empty_dir").is_dir());
}

#[test]
fn nested_empty_directories_are_created() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("a").join("b").join("c").is_dir());
}

// ── 11. Large files ─────────────────────────────────────────────────────

#[test]
fn large_file_copies_correctly() {
    let src = tempfile::tempdir().unwrap();
    // 1 MiB of data
    let data = vec![0xABu8; 1024 * 1024];
    fs::write(src.path().join("large.bin"), &data).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = fs::read(ws.path().join("large.bin")).unwrap();
    assert_eq!(staged.len(), data.len());
    assert_eq!(staged, data);
}

#[test]
fn multiple_large_files_copy_correctly() {
    let src = tempfile::tempdir().unwrap();
    for i in 0..3 {
        let data = vec![i as u8; 512 * 1024];
        fs::write(src.path().join(format!("large_{i}.bin")), &data).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    for i in 0..3 {
        let staged = fs::read(ws.path().join(format!("large_{i}.bin"))).unwrap();
        assert_eq!(staged.len(), 512 * 1024);
        assert!(staged.iter().all(|&b| b == i as u8));
    }
}

// ── 12. Hidden files ────────────────────────────────────────────────────

#[test]
fn hidden_files_are_copied_without_filters() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    fs::write(src.path().join("visible.txt"), "public").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(".hidden").is_file());
    assert!(ws.path().join("visible.txt").is_file());
}

#[test]
fn hidden_files_filtered_by_exclude_glob() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join(".env"), "SECRET=123").unwrap();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    fs::write(src.path().join("main.rs"), "fn main(){}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&[".env"]))
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".env").exists());
    assert!(ws.path().join(".gitignore").is_file());
    assert!(ws.path().join("main.rs").is_file());
}

#[test]
fn hidden_directory_copied_without_filters() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join(".config")).unwrap();
    fs::write(src.path().join(".config").join("settings.json"), "{}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(".config").join("settings.json").is_file());
}

// ── 13. Unicode filenames ───────────────────────────────────────────────

#[test]
fn unicode_filename_copies_correctly() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("données.txt"), "contenu français").unwrap();
    fs::write(src.path().join("日本語.txt"), "日本語の内容").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("données.txt")).unwrap(),
        "contenu français"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("日本語.txt")).unwrap(),
        "日本語の内容"
    );
}

#[test]
fn unicode_directory_name_preserved() {
    let src = tempfile::tempdir().unwrap();
    let dir = src.path().join("données");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("fichier.rs"), "fn données() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = ws.path().join("données").join("fichier.rs");
    assert!(staged.is_file());
    assert_eq!(fs::read_to_string(staged).unwrap(), "fn données() {}");
}

#[test]
fn unicode_in_glob_patterns_works() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("données.txt"), "yes").unwrap();
    fs::write(src.path().join("other.txt"), "no").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["données*"]))
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("données.txt").is_file());
    assert!(!ws.path().join("other.txt").exists());
}

// ── 14. Read-only files ─────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn readonly_file_copies_successfully() {
    use std::os::unix::fs::PermissionsExt;

    let src = tempfile::tempdir().unwrap();
    let file_path = src.path().join("readonly.txt");
    fs::write(&file_path, "locked content").unwrap();
    fs::set_permissions(&file_path, fs::Permissions::from_mode(0o444)).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = ws.path().join("readonly.txt");
    assert!(staged.is_file());
    assert_eq!(fs::read_to_string(&staged).unwrap(), "locked content");

    // Restore write permissions for cleanup.
    fs::set_permissions(&file_path, fs::Permissions::from_mode(0o644)).unwrap();
}

#[cfg(windows)]
#[test]
fn readonly_file_copies_successfully() {
    let src = tempfile::tempdir().unwrap();
    let file_path = src.path().join("readonly.txt");
    fs::write(&file_path, "locked content").unwrap();

    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file_path, perms).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = ws.path().join("readonly.txt");
    assert!(staged.is_file());
    assert_eq!(fs::read_to_string(&staged).unwrap(), "locked content");

    // Restore write permissions for cleanup — allow clippy lint since this is
    // a Windows-only test and the Unix caveat does not apply.
    #[allow(clippy::permissions_set_readonly_false)]
    {
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_readonly(false);
        fs::set_permissions(&file_path, perms).unwrap();
    }
}

// ── 15. Cleanup ─────────────────────────────────────────────────────────

#[test]
fn staging_area_cleaned_up_on_drop() {
    let src = make_source_tree();
    let path_copy;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();
        path_copy = ws.path().to_path_buf();
        assert!(path_copy.exists());
    }
    // After `ws` is dropped, the temp directory should be cleaned up.
    assert!(
        !path_copy.exists(),
        "staging dir should be cleaned up on drop"
    );
}

#[test]
fn passthrough_mode_does_not_create_temp_dir() {
    let src = make_source_tree();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // PassThrough returns the original path, no temp dir.
    assert_eq!(ws.path(), src.path());
}

// ── Additional edge-case tests ──────────────────────────────────────────

#[test]
fn staging_empty_source_directory() {
    let src = tempfile::tempdir().unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

#[test]
fn staging_with_no_source_root_errors() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("source_root"),
        "error should mention source_root: {err}"
    );
}

#[test]
fn staging_nonexistent_source_errors() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/12345")
        .stage();
    assert!(result.is_err());
}

#[test]
fn file_with_special_characters_in_name() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("file with spaces.txt"), "spaced").unwrap();
    fs::write(src.path().join("file-with-dashes.txt"), "dashed").unwrap();
    fs::write(src.path().join("file_with_underscores.txt"), "under").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("file with spaces.txt").is_file());
    assert!(ws.path().join("file-with-dashes.txt").is_file());
    assert!(ws.path().join("file_with_underscores.txt").is_file());
}

#[test]
fn exclude_wildcard_pattern_works() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("app.log"), "log data").unwrap();
    fs::write(src.path().join("debug.log"), "debug data").unwrap();
    fs::write(src.path().join("main.rs"), "fn main(){}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["*.log"]))
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join("app.log").exists());
    assert!(!ws.path().join("debug.log").exists());
    assert!(ws.path().join("main.rs").is_file());
}

#[test]
fn binary_file_integrity_preserved() {
    let src = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("binary.bin"), &data).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = fs::read(ws.path().join("binary.bin")).unwrap();
    assert_eq!(staged, data);
}

#[test]
fn empty_file_copies_correctly() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = ws.path().join("empty.txt");
    assert!(staged.is_file());
    assert_eq!(fs::read_to_string(staged).unwrap(), "");
}

#[test]
fn staging_preserves_file_count() {
    let src = tempfile::tempdir().unwrap();
    let file_count = 20;
    for i in 0..file_count {
        fs::write(
            src.path().join(format!("file_{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let count = fs::read_dir(ws.path())
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_file())
        .count();
    assert_eq!(count, file_count);
}

#[test]
fn git_init_does_not_affect_file_content() {
    let src = make_source_tree();

    let with_git = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    let without_git = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let content_with = fs::read_to_string(with_git.path().join("README.md")).unwrap();
    let content_without = fs::read_to_string(without_git.path().join("README.md")).unwrap();
    assert_eq!(content_with, content_without);
}

#[test]
fn exclude_nested_directory_pattern() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join("src").join("generated")).unwrap();
    fs::write(
        src.path().join("src").join("generated").join("out.rs"),
        "generated",
    )
    .unwrap();
    fs::write(src.path().join("src").join("main.rs"), "fn main(){}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["src/generated/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("src").join("main.rs").is_file());
    assert!(!ws
        .path()
        .join("src")
        .join("generated")
        .join("out.rs")
        .exists());
}

fn count_files_recursive(path: &Path) -> usize {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count()
}

#[test]
fn staged_file_count_matches_filtered_source() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join("keep")).unwrap();
    fs::write(src.path().join("keep").join("a.rs"), "a").unwrap();
    fs::write(src.path().join("keep").join("b.rs"), "b").unwrap();
    fs::write(src.path().join("skip.log"), "log").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["keep/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(count_files_recursive(ws.path()), 2);
}

#[test]
fn diff_after_adding_file_shows_new_content() {
    let src = make_source_tree();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // Add and stage a new file.
    fs::write(ws.path().join("added.txt"), "new file content").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "added.txt"])
        .current_dir(ws.path())
        .status();

    let diff = std::process::Command::new("git")
        .args(["diff", "--cached", "--no-color"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let diff_text = String::from_utf8_lossy(&diff.stdout);

    assert!(
        diff_text.contains("new file content"),
        "cached diff should show new file: {diff_text}"
    );
}

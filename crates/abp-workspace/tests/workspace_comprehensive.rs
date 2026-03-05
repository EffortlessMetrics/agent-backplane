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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for abp-workspace: staging, git operations, diff
//! generation, glob filtering, cleanup, edge cases, permissions, symlinks,
//! large files, and concurrent staging.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{
    ChangeType, DiffAnalyzer, DiffPolicy, DiffSummary, FileChange, PolicyResult, WorkspaceDiff,
    diff_workspace,
};
use abp_workspace::{PreparedWorkspace, WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ===========================================================================
// Helpers
// ===========================================================================

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

fn passthrough_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    }
}

fn git_cmd(dir: &Path, args: &[&str]) -> String {
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

/// Collect sorted relative file paths under `root`, excluding `.git`.
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

fn stage_from(src: &Path) -> PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .stage()
        .expect("staging should succeed")
}

// ===========================================================================
// 1. Staging — copy files to temp dir
// ===========================================================================

#[test]
fn staging_copies_files_to_temp_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "alpha").unwrap();
    fs::write(src.path().join("b.txt"), "beta").unwrap();

    let ws = stage_from(src.path());

    assert_ne!(ws.path(), src.path());
    assert_eq!(
        fs::read_to_string(ws.path().join("a.txt")).unwrap(),
        "alpha"
    );
    assert_eq!(fs::read_to_string(ws.path().join("b.txt")).unwrap(), "beta");
}

#[test]
fn staging_preserves_file_contents_exactly() {
    let src = tempdir().unwrap();
    let content = "line1\nline2\r\nline3\ttab\0null";
    fs::write(src.path().join("mixed.bin"), content).unwrap();

    let ws = stage_from(src.path());
    assert_eq!(
        fs::read(ws.path().join("mixed.bin")).unwrap(),
        content.as_bytes()
    );
}

#[test]
fn staging_copies_nested_directories() {
    let src = tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.txt"), "deep").unwrap();
    fs::write(src.path().join("root.txt"), "root").unwrap();

    let ws = stage_from(src.path());
    assert!(
        ws.path()
            .join("a")
            .join("b")
            .join("c")
            .join("deep.txt")
            .exists()
    );
    assert!(ws.path().join("root.txt").exists());
}

#[test]
fn staging_excludes_dot_git_directory() {
    let src = tempdir().unwrap();
    let git_dir = src.path().join(".git");
    fs::create_dir_all(git_dir.join("objects")).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The source .git content should not be copied
    let files = collect_files(ws.path());
    assert_eq!(files, vec!["file.txt"]);
}

#[test]
fn staging_via_workspace_manager() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "world").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert_ne!(ws.path(), src.path());
}

// ===========================================================================
// 2. Git initialization
// ===========================================================================

#[test]
fn staged_workspace_has_git_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = stage_from(src.path());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn staged_workspace_has_initial_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = stage_from(src.path());
    let log = git_cmd(ws.path(), &["log", "--oneline"]);
    assert!(!log.is_empty(), "should have at least one commit");
}

#[test]
fn fresh_staged_workspace_is_clean() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = stage_from(src.path());
    let status = git_cmd(ws.path(), &["status", "--porcelain=v1"]);
    assert!(
        status.is_empty(),
        "fresh workspace should be clean, got: {status}"
    );
}

#[test]
fn git_init_disabled_means_no_git_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
}

#[test]
fn all_staged_files_committed_in_baseline() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("c.txt"), "c").unwrap();

    let ws = stage_from(src.path());
    let committed = git_cmd(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    let mut lines: Vec<&str> = committed.lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["a.txt", "b.txt", "sub/c.txt"]);
}

// ===========================================================================
// 3. Diff generation
// ===========================================================================

#[test]
fn diff_detects_added_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("seed.txt"), "seed").unwrap();

    let ws = stage_from(src.path());
    fs::write(ws.path().join("new.txt"), "new content\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.added.contains(&PathBuf::from("new.txt")));
    assert_eq!(summary.total_additions, 1);
}

#[test]
fn diff_detects_modified_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "old\n").unwrap();

    let ws = stage_from(src.path());
    fs::write(ws.path().join("data.txt"), "new\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.modified.contains(&PathBuf::from("data.txt")));
    assert!(summary.total_additions >= 1);
    assert!(summary.total_deletions >= 1);
}

#[test]
fn diff_detects_deleted_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "bye\n").unwrap();

    let ws = stage_from(src.path());
    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.deleted.contains(&PathBuf::from("doomed.txt")));
}

#[test]
fn diff_no_changes_is_empty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("stable.txt"), "unchanged").unwrap();

    let ws = stage_from(src.path());
    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

#[test]
fn diff_mixed_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("modify.txt"), "old\n").unwrap();
    fs::write(src.path().join("delete.txt"), "gone\n").unwrap();
    fs::write(src.path().join("keep.txt"), "safe\n").unwrap();

    let ws = stage_from(src.path());
    fs::write(ws.path().join("modify.txt"), "new\n").unwrap();
    fs::remove_file(ws.path().join("delete.txt")).unwrap();
    fs::write(ws.path().join("added.txt"), "fresh\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.file_count(), 3);
    assert!(summary.added.contains(&PathBuf::from("added.txt")));
    assert!(summary.modified.contains(&PathBuf::from("modify.txt")));
    assert!(summary.deleted.contains(&PathBuf::from("delete.txt")));
}

#[test]
fn diff_nested_file_paths_use_forward_slash() {
    let src = tempdir().unwrap();
    let nested = src.path().join("sub").join("dir");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("file.txt"), "hello").unwrap();

    let ws = stage_from(src.path());
    fs::write(
        ws.path().join("sub").join("dir").join("file.txt"),
        "changed",
    )
    .unwrap();

    let summary = diff_workspace(&ws).unwrap();
    // Git uses forward slashes in paths regardless of OS
    assert!(
        summary
            .modified
            .contains(&PathBuf::from("sub/dir/file.txt"))
    );
}

// ===========================================================================
// 4. Glob filtering
// ===========================================================================

#[test]
fn include_glob_filters_to_matching_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("readme.md"), "# Title").unwrap();
    fs::write(src.path().join("data.log"), "log entry").unwrap();

    let spec = staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("code.rs").exists());
    assert!(!ws.path().join("readme.md").exists());
    assert!(!ws.path().join("data.log").exists());
}

#[test]
fn exclude_glob_removes_matching_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("skip.log"), "log").unwrap();
    fs::write(src.path().join("also.txt"), "text").unwrap();

    let spec = staged_spec_globs(src.path(), vec![], vec!["*.log".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("keep.rs").exists());
    assert!(!ws.path().join("skip.log").exists());
    assert!(ws.path().join("also.txt").exists());
}

#[test]
fn include_and_exclude_combined() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("main.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("test.rs"), "// test").unwrap();
    fs::write(src.path().join("readme.md"), "# Doc").unwrap();

    let spec = staged_spec_globs(src.path(), vec!["*.rs".into()], vec!["test.*".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("main.rs").exists());
    assert!(!ws.path().join("test.rs").exists());
    assert!(!ws.path().join("readme.md").exists());
}

#[test]
fn glob_filters_nested_directory_files() {
    let src = tempdir().unwrap();
    let sub = src.path().join("src");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("lib.rs"), "pub mod x;").unwrap();
    fs::write(sub.join("data.csv"), "a,b,c").unwrap();
    fs::write(src.path().join("build.log"), "ok").unwrap();

    let spec = staged_spec_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("src").join("lib.rs").exists());
    assert!(!ws.path().join("src").join("data.csv").exists());
    assert!(!ws.path().join("build.log").exists());
}

#[test]
fn exclude_glob_with_directory_pattern() {
    let src = tempdir().unwrap();
    let target = src.path().join("target").join("debug");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("binary"), "ELF").unwrap();
    fs::write(src.path().join("src.rs"), "fn main(){}").unwrap();

    let spec = staged_spec_globs(src.path(), vec![], vec!["target/**".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("src.rs").exists());
    assert!(
        !ws.path()
            .join("target")
            .join("debug")
            .join("binary")
            .exists()
    );
}

#[test]
fn glob_dotfiles_excluded() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".env"), "SECRET=x").unwrap();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    fs::write(src.path().join("app.rs"), "fn main(){}").unwrap();

    let spec = staged_spec_globs(src.path(), vec![], vec![".*".into()]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("app.rs").exists());
    assert!(!ws.path().join(".env").exists());
    assert!(!ws.path().join(".gitignore").exists());
}

#[test]
fn glob_multiple_extensions_include() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "rust").unwrap();
    fs::write(src.path().join("b.toml"), "toml").unwrap();
    fs::write(src.path().join("c.json"), "json").unwrap();
    fs::write(src.path().join("d.txt"), "text").unwrap();

    let spec = staged_spec_globs(src.path(), vec!["*.rs".into(), "*.toml".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("a.rs").exists());
    assert!(ws.path().join("b.toml").exists());
    assert!(!ws.path().join("c.json").exists());
    assert!(!ws.path().join("d.txt").exists());
}

#[test]
fn stager_builder_include_exclude() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "keep").unwrap();
    fs::write(src.path().join("skip.log"), "skip").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .exclude(vec![])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("keep.rs").exists());
    assert!(!ws.path().join("skip.log").exists());
}

// ===========================================================================
// 5. Cleanup — temp directory removed on drop
// ===========================================================================

#[test]
fn staged_workspace_cleanup_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = stage_from(src.path());
    let ws_path = ws.path().to_path_buf();
    assert!(ws_path.exists());

    drop(ws);
    assert!(!ws_path.exists(), "temp dir should be cleaned up on drop");
}

#[test]
fn passthrough_workspace_does_not_delete_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    let ws_path = ws.path().to_path_buf();
    drop(ws);

    assert!(ws_path.exists(), "passthrough should not delete source dir");
    assert!(ws_path.join("file.txt").exists());
}

#[test]
fn multiple_staged_workspaces_independent_cleanup() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws1 = stage_from(src.path());
    let ws2 = stage_from(src.path());
    let p1 = ws1.path().to_path_buf();
    let p2 = ws2.path().to_path_buf();

    drop(ws1);
    assert!(!p1.exists());
    assert!(p2.exists(), "ws2 should still exist");
    drop(ws2);
    assert!(!p2.exists());
}

// ===========================================================================
// 6. Edge cases
// ===========================================================================

#[test]
fn empty_source_directory_stages_successfully() {
    let src = tempdir().unwrap();
    // No files at all
    let ws = stage_from(src.path());
    assert!(ws.path().exists());
}

#[test]
fn source_with_only_dot_git_copies_nothing() {
    let src = tempdir().unwrap();
    let git_dir = src.path().join(".git");
    fs::create_dir_all(git_dir.join("objects")).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(
        files.is_empty(),
        "only .git existed, nothing should be copied"
    );
}

#[test]
fn very_deep_nesting_staged() {
    let src = tempdir().unwrap();
    let mut deep = src.path().to_path_buf();
    for i in 0..15 {
        deep = deep.join(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "deep leaf").unwrap();

    let ws = stage_from(src.path());

    let mut check = ws.path().to_path_buf();
    for i in 0..15 {
        check = check.join(format!("d{i}"));
    }
    assert!(check.join("leaf.txt").exists());
    assert_eq!(
        fs::read_to_string(check.join("leaf.txt")).unwrap(),
        "deep leaf"
    );
}

#[test]
fn staging_source_with_empty_subdirs() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_sub")).unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = stage_from(src.path());
    assert!(ws.path().join("file.txt").exists());
    // Empty directories may or may not be copied; just ensure no crash
}

#[test]
fn stager_error_without_source_root() {
    let err = WorkspaceStager::new().stage();
    assert!(err.is_err());
}

#[test]
fn stager_error_nonexistent_source() {
    let err = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist/ever")
        .stage();
    assert!(err.is_err());
}

#[test]
fn passthrough_mode_uses_original_path() {
    let src = tempdir().unwrap();
    let spec = passthrough_spec(src.path());
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn staging_file_with_special_characters_in_name() {
    let src = tempdir().unwrap();
    // Spaces and hyphens in filename
    fs::write(src.path().join("my file-name.txt"), "special").unwrap();

    let ws = stage_from(src.path());
    assert!(ws.path().join("my file-name.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("my file-name.txt")).unwrap(),
        "special"
    );
}

#[test]
fn staging_empty_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();

    let ws = stage_from(src.path());
    assert!(ws.path().join("empty.txt").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

#[test]
fn staging_unicode_file_content() {
    let src = tempdir().unwrap();
    let content = "こんにちは世界 🌍 — émojis & ñ";
    fs::write(src.path().join("unicode.txt"), content).unwrap();

    let ws = stage_from(src.path());
    assert_eq!(
        fs::read_to_string(ws.path().join("unicode.txt")).unwrap(),
        content
    );
}

// ===========================================================================
// 7. File permissions (unix only)
// ===========================================================================

#[cfg(unix)]
mod unix_permissions {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn read_only_files_are_staged() {
        let src = tempdir().unwrap();
        let path = src.path().join("readonly.txt");
        fs::write(&path, "locked content").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&path, perms).unwrap();

        let ws = stage_from(src.path());
        assert!(ws.path().join("readonly.txt").exists());
        assert_eq!(
            fs::read_to_string(ws.path().join("readonly.txt")).unwrap(),
            "locked content"
        );
    }

    #[test]
    fn executable_bit_file_is_staged() {
        let src = tempdir().unwrap();
        let path = src.path().join("script.sh");
        fs::write(&path, "#!/bin/bash\necho hello").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();

        let ws = stage_from(src.path());
        assert!(ws.path().join("script.sh").exists());
    }
}

// ===========================================================================
// 8. Symlinks
// ===========================================================================

#[cfg(unix)]
mod unix_symlinks {
    use super::*;
    use std::os::unix::fs as unix_fs;

    #[test]
    fn symlink_to_file_not_followed_during_staging() {
        let src = tempdir().unwrap();
        let real = src.path().join("real.txt");
        fs::write(&real, "target content").unwrap();
        unix_fs::symlink(&real, src.path().join("link.txt")).unwrap();

        // Staging uses follow_links(false), so symlinks are not followed as
        // regular files. The real file should always be staged.
        let ws = stage_from(src.path());
        assert!(ws.path().join("real.txt").exists());
    }

    #[test]
    fn symlink_to_directory_not_followed() {
        let src = tempdir().unwrap();
        let real_dir = src.path().join("real_dir");
        fs::create_dir_all(&real_dir).unwrap();
        fs::write(real_dir.join("inner.txt"), "inside").unwrap();
        unix_fs::symlink(&real_dir, src.path().join("link_dir")).unwrap();

        let ws = stage_from(src.path());
        // Real dir content should be staged
        assert!(ws.path().join("real_dir").join("inner.txt").exists());
        // Symlink dir should not be followed
        assert!(!ws.path().join("link_dir").join("inner.txt").exists());
    }

    #[test]
    fn broken_symlink_does_not_crash_staging() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("good.txt"), "ok").unwrap();
        unix_fs::symlink("/nonexistent/path", src.path().join("broken_link")).unwrap();

        let ws = stage_from(src.path());
        assert!(ws.path().join("good.txt").exists());
    }
}

#[cfg(windows)]
mod windows_symlinks {
    use super::*;

    #[test]
    fn staging_without_symlinks_works() {
        // On Windows, symlink creation usually requires admin privileges.
        // Simply verify staging works normally.
        let src = tempdir().unwrap();
        fs::write(src.path().join("file.txt"), "content").unwrap();
        let ws = stage_from(src.path());
        assert!(ws.path().join("file.txt").exists());
    }
}

// ===========================================================================
// 9. Large files
// ===========================================================================

#[test]
fn large_file_staged_correctly() {
    let src = tempdir().unwrap();
    // ~500 KB file
    let content: String = (0..10_000)
        .map(|i| format!("line {i}: padding data here\n"))
        .collect();
    fs::write(src.path().join("large.txt"), &content).unwrap();

    let ws = stage_from(src.path());
    assert_eq!(
        fs::read_to_string(ws.path().join("large.txt")).unwrap(),
        content
    );
}

#[test]
fn large_binary_file_staged_correctly() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = (0..100_000u32).flat_map(|i| i.to_le_bytes()).collect();
    fs::write(src.path().join("large.bin"), &data).unwrap();

    let ws = stage_from(src.path());
    assert_eq!(fs::read(ws.path().join("large.bin")).unwrap(), data);
}

#[test]
fn large_file_diff_after_modification() {
    let src = tempdir().unwrap();
    let original: String = (0..1_000).map(|i| format!("line {i}\n")).collect();
    fs::write(src.path().join("big.txt"), &original).unwrap();

    let ws = stage_from(src.path());
    let modified: String = (0..1_000).map(|i| format!("CHANGED {i}\n")).collect();
    fs::write(ws.path().join("big.txt"), &modified).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.modified.contains(&PathBuf::from("big.txt")));
    assert!(summary.total_additions >= 1000);
    assert!(summary.total_deletions >= 1000);
}

// ===========================================================================
// 10. Concurrent staging — multiple workspaces from same source
// ===========================================================================

#[test]
fn concurrent_staging_from_same_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "data").unwrap();

    let ws1 = stage_from(src.path());
    let ws2 = stage_from(src.path());
    let ws3 = stage_from(src.path());

    // All should have independent copies
    assert_ne!(ws1.path(), ws2.path());
    assert_ne!(ws2.path(), ws3.path());

    // Mutating one does not affect others
    fs::write(ws1.path().join("shared.txt"), "mutated").unwrap();
    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "data"
    );
    assert_eq!(
        fs::read_to_string(ws3.path().join("shared.txt")).unwrap(),
        "data"
    );
}

#[test]
fn concurrent_staging_independent_git_repos() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original").unwrap();

    let ws1 = stage_from(src.path());
    let ws2 = stage_from(src.path());

    // Modify ws1 only
    fs::write(ws1.path().join("file.txt"), "changed in ws1").unwrap();

    let s1 = diff_workspace(&ws1).unwrap();
    let s2 = diff_workspace(&ws2).unwrap();

    assert!(!s1.is_empty(), "ws1 should have changes");
    assert!(s2.is_empty(), "ws2 should be clean");
}

#[test]
fn concurrent_staging_with_threads() {
    let src = tempdir().unwrap();
    for i in 0..5 {
        fs::write(src.path().join(format!("f{i}.txt")), format!("data{i}")).unwrap();
    }
    let src_path = src.path().to_path_buf();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let p = src_path.clone();
            std::thread::spawn(move || {
                let ws = WorkspaceStager::new().source_root(&p).stage().unwrap();
                // Verify all files exist
                for i in 0..5 {
                    assert!(ws.path().join(format!("f{i}.txt")).exists());
                }
                ws.path().to_path_buf()
            })
        })
        .collect();

    let paths: Vec<PathBuf> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All paths should be unique
    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            assert_ne!(paths[i], paths[j]);
        }
    }
}

// ===========================================================================
// DiffAnalyzer tests
// ===========================================================================

#[test]
fn diff_analyzer_no_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = stage_from(src.path());
    let analyzer = DiffAnalyzer::new(ws.path());

    assert!(!analyzer.has_changes());
    assert!(analyzer.changed_files().is_empty());
}

#[test]
fn diff_analyzer_detects_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original\n").unwrap();

    let ws = stage_from(src.path());
    fs::write(ws.path().join("file.txt"), "modified\n").unwrap();
    fs::write(ws.path().join("new.txt"), "added\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.has_changes());

    let changed = analyzer.changed_files();
    assert!(changed.contains(&PathBuf::from("file.txt")));
    assert!(changed.contains(&PathBuf::from("new.txt")));
}

#[test]
fn diff_analyzer_file_was_modified() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "old\n").unwrap();
    fs::write(src.path().join("b.txt"), "keep\n").unwrap();

    let ws = stage_from(src.path());
    fs::write(ws.path().join("a.txt"), "new\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.file_was_modified(Path::new("a.txt")));
    assert!(!analyzer.file_was_modified(Path::new("b.txt")));
}

#[test]
fn diff_analyzer_analyze_returns_workspace_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("mod.txt"), "old\n").unwrap();

    let ws = stage_from(src.path());
    fs::write(ws.path().join("mod.txt"), "new\n").unwrap();
    fs::write(ws.path().join("add.txt"), "added\n").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    assert!(!diff.is_empty());
    assert_eq!(diff.files_modified.len(), 1);
    assert_eq!(diff.files_added.len(), 1);
    assert_eq!(diff.files_modified[0].change_type, ChangeType::Modified);
    assert_eq!(diff.files_added[0].change_type, ChangeType::Added);
}

// ===========================================================================
// WorkspaceDiff tests
// ===========================================================================

#[test]
fn workspace_diff_summary_no_changes() {
    let diff = WorkspaceDiff::default();
    assert_eq!(diff.summary(), "No changes detected.");
    assert!(diff.is_empty());
    assert_eq!(diff.file_count(), 0);
}

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
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 10,
        total_deletions: 0,
    };
    let summary = diff.summary();
    assert!(summary.contains("1 file(s) changed"));
    assert!(summary.contains("1 added"));
    assert!(summary.contains("+10"));
}

// ===========================================================================
// DiffPolicy tests
// ===========================================================================

#[test]
fn diff_policy_pass_when_within_limits() {
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("a.txt"),
            change_type: ChangeType::Added,
            additions: 5,
            deletions: 0,
            is_binary: false,
        }],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 5,
        total_deletions: 0,
    };

    let policy = DiffPolicy {
        max_files: Some(10),
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
        ],
        files_modified: vec![],
        files_deleted: vec![],
        total_additions: 2,
        total_deletions: 0,
    };

    let policy = DiffPolicy {
        max_files: Some(1),
        max_additions: None,
        denied_paths: vec![],
    };

    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations[0].contains("too many files"));
    }
}

#[test]
fn diff_policy_fail_too_many_additions() {
    let diff = WorkspaceDiff {
        files_added: vec![FileChange {
            path: PathBuf::from("big.txt"),
            change_type: ChangeType::Added,
            additions: 500,
            deletions: 0,
            is_binary: false,
        }],
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
}

#[test]
fn diff_policy_denied_paths() {
    let diff = WorkspaceDiff {
        files_modified: vec![FileChange {
            path: PathBuf::from("secret.key"),
            change_type: ChangeType::Modified,
            additions: 1,
            deletions: 1,
            is_binary: false,
        }],
        files_added: vec![],
        files_deleted: vec![],
        total_additions: 1,
        total_deletions: 1,
    };

    let policy = DiffPolicy {
        max_files: None,
        max_additions: None,
        denied_paths: vec!["*.key".into()],
    };

    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations[0].contains("denied path"));
    }
}

// ===========================================================================
// DiffSummary struct tests
// ===========================================================================

#[test]
fn diff_summary_default_is_empty() {
    let s = DiffSummary::default();
    assert!(s.is_empty());
    assert_eq!(s.file_count(), 0);
    assert_eq!(s.total_changes(), 0);
}

#[test]
fn diff_summary_file_count_and_total_changes() {
    let s = DiffSummary {
        added: vec![PathBuf::from("a"), PathBuf::from("b")],
        modified: vec![PathBuf::from("c")],
        deleted: vec![PathBuf::from("d")],
        total_additions: 20,
        total_deletions: 5,
    };
    assert_eq!(s.file_count(), 4);
    assert_eq!(s.total_changes(), 25);
    assert!(!s.is_empty());
}

#[test]
fn diff_summary_serde_roundtrip() {
    let s = DiffSummary {
        added: vec![PathBuf::from("new.rs")],
        modified: vec![],
        deleted: vec![PathBuf::from("old.rs")],
        total_additions: 10,
        total_deletions: 3,
    };
    let json = serde_json::to_string(&s).unwrap();
    let deserialized: DiffSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, deserialized);
}

// ===========================================================================
// WorkspaceManager git_status / git_diff
// ===========================================================================

#[test]
fn workspace_manager_git_status_clean() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(status.unwrap().trim().is_empty());
}

#[test]
fn workspace_manager_git_status_dirty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("file.txt"), "modified").unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(!status.unwrap().trim().is_empty());
}

#[test]
fn workspace_manager_git_diff_empty_on_clean() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    assert!(diff.unwrap().trim().is_empty());
}

#[test]
fn workspace_manager_git_diff_nonempty_after_change() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("file.txt"), "changed").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    let diff_text = diff.unwrap();
    assert!(diff_text.contains("file.txt"));
}

// ===========================================================================
// Additional edge cases and integration tests
// ===========================================================================

#[test]
fn staging_many_files() {
    let src = tempdir().unwrap();
    for i in 0..100 {
        fs::write(
            src.path().join(format!("file_{i:03}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let ws = stage_from(src.path());
    for i in 0..100 {
        assert!(ws.path().join(format!("file_{i:03}.txt")).exists());
    }
}

#[test]
fn diff_after_adding_many_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("seed.txt"), "seed").unwrap();

    let ws = stage_from(src.path());
    for i in 0..20 {
        fs::write(
            ws.path().join(format!("added_{i}.txt")),
            format!("new content {i}\n"),
        )
        .unwrap();
    }

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 20);
    assert_eq!(summary.total_additions, 20);
}

#[test]
fn diff_binary_files_no_line_counts() {
    let src = tempdir().unwrap();
    // Use a full PNG header + enough NUL bytes so git classifies it as binary
    let mut png = vec![
        0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    ];
    png.extend_from_slice(&[0x00; 256]);
    fs::write(src.path().join("img.png"), &png).unwrap();

    let ws = stage_from(src.path());
    let mut modified = png;
    modified.extend_from_slice(&[0xDE, 0xAD, 0x00, 0x00]);
    fs::write(ws.path().join("img.png"), &modified).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.modified.contains(&PathBuf::from("img.png")));
    // Binary files produce 0 line-level additions/deletions
    assert_eq!(summary.total_additions, 0);
    assert_eq!(summary.total_deletions, 0);
}

#[test]
fn change_type_display() {
    assert_eq!(format!("{}", ChangeType::Added), "added");
    assert_eq!(format!("{}", ChangeType::Modified), "modified");
    assert_eq!(format!("{}", ChangeType::Deleted), "deleted");
}

#[test]
fn policy_result_is_pass() {
    assert!(PolicyResult::Pass.is_pass());
    assert!(
        !PolicyResult::Fail {
            violations: vec!["oops".into()]
        }
        .is_pass()
    );
}

#[test]
fn workspace_stager_default_is_new() {
    let stager = WorkspaceStager::default();
    // Should be equivalent to new() — no source root set, so staging fails
    let result = stager.stage();
    assert!(result.is_err());
}

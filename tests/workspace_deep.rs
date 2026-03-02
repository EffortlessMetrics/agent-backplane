// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep workspace staging tests covering edge cases and runtime integration.
//!
//! Covers symlink handling, large directory trees, hidden files, concurrent
//! workspace creation, cleanup on error, diff generation, complex glob
//! patterns, re-staging, and path traversal prevention.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::diff_workspace;
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Collect sorted relative file paths (excluding `.git`) under `root`.
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

/// Run a git command in `dir` and return trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ===========================================================================
// 1. Symlink handling (follow vs skip)
// ===========================================================================

#[test]
fn symlink_to_file_is_skipped_not_followed() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real content").unwrap();

    // Create a symlink; skip the test gracefully on platforms where it fails.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src.path().join("real.txt"), src.path().join("link.txt"))
            .unwrap();
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(
            src.path().join("real.txt"),
            src.path().join("link.txt"),
        )
        .is_err()
        {
            // Symlinks require elevated privileges on Windows; skip.
            return;
        }
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("real.txt").exists());
    // The symlink should NOT be copied (copy_workspace uses follow_links(false)
    // and only copies is_file() entries, symlinks are neither is_file nor is_dir).
    assert!(
        !ws.path().join("link.txt").exists(),
        "symlink should be skipped, not followed"
    );
}

#[test]
fn symlink_to_directory_is_skipped() {
    let src = tempdir().unwrap();
    let subdir = src.path().join("realdir");
    fs::create_dir_all(&subdir).unwrap();
    fs::write(subdir.join("inner.txt"), "inner").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&subdir, src.path().join("linkdir")).unwrap();
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(&subdir, src.path().join("linkdir")).is_err() {
            return;
        }
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("realdir").join("inner.txt").exists());
    // The symlinked directory should not be traversed.
    assert!(
        !ws.path().join("linkdir").exists(),
        "symlinked directory should be skipped"
    );
}

#[test]
fn dangling_symlink_does_not_cause_error() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "keep").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/nonexistent/target", src.path().join("dangling.txt")).unwrap();
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(
            "Z:\\nonexistent\\target",
            src.path().join("dangling.txt"),
        )
        .is_err()
        {
            return;
        }
    }

    // Staging must succeed even with a dangling symlink.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("keep.txt").exists());
    assert!(!ws.path().join("dangling.txt").exists());
}

// ===========================================================================
// 2. Large directory trees (1000+ files)
// ===========================================================================

#[test]
fn large_directory_tree_1000_files() {
    let src = tempdir().unwrap();
    let count = 1000;
    // Spread across multiple directories.
    for i in 0..count {
        let dir = src.path().join(format!("dir_{:02}", i % 50));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("file_{i:04}.txt")), format!("data {i}")).unwrap();
    }

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let staged_files = collect_files(ws.path());
    assert_eq!(
        staged_files.len(),
        count,
        "all {count} files must be staged across directories"
    );
}

#[test]
fn large_tree_with_deep_nesting() {
    let src = tempdir().unwrap();
    // Create 20 directories, each with 50+ files at varying depths.
    for depth in 0..20 {
        let mut dir = src.path().to_path_buf();
        for d in 0..=depth {
            dir = dir.join(format!("level{d}"));
        }
        fs::create_dir_all(&dir).unwrap();
        for f in 0..50 {
            fs::write(dir.join(format!("f{f}.txt")), format!("d{depth}f{f}")).unwrap();
        }
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let src_files = collect_files(src.path());
    let ws_files = collect_files(ws.path());
    assert_eq!(src_files.len(), ws_files.len());
    assert_eq!(src_files, ws_files);
}

// ===========================================================================
// 3. Hidden files and dot-directories
// ===========================================================================

#[test]
fn hidden_dotfiles_are_staged() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    fs::write(src.path().join(".env"), "VAR=1").unwrap();
    fs::write(src.path().join("visible.txt"), "visible").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(".hidden").exists());
    assert!(ws.path().join(".env").exists());
    assert!(ws.path().join("visible.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join(".hidden")).unwrap(),
        "secret"
    );
}

#[test]
fn dot_directories_are_staged_except_dot_git() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".config")).unwrap();
    fs::write(src.path().join(".config").join("settings.json"), "{}").unwrap();
    fs::create_dir_all(src.path().join(".vscode")).unwrap();
    fs::write(
        src.path().join(".vscode").join("launch.json"),
        r#"{"version":"0.2.0"}"#,
    )
    .unwrap();
    // .git should be excluded by the walker
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(".config").join("settings.json").exists());
    assert!(ws.path().join(".vscode").join("launch.json").exists());
    assert!(
        !ws.path().join(".git").exists(),
        ".git should never be copied"
    );
}

#[test]
fn hidden_files_can_be_excluded_via_glob() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".env"), "SECRET=1").unwrap();
    fs::write(src.path().join(".hidden"), "hidden").unwrap();
    fs::write(src.path().join("code.rs"), "fn main(){}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec![".*".into()]))
        .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with('.')));
    assert!(files.contains(&"code.rs".to_string()));
}

// ===========================================================================
// 4. File permission preservation (Unix-only)
// ===========================================================================

#[cfg(unix)]
#[test]
fn file_permissions_preserved_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    let src = tempdir().unwrap();
    let script = src.path().join("run.sh");
    fs::write(&script, "#!/bin/sh\necho hello").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    let regular = src.path().join("data.txt");
    fs::write(&regular, "data").unwrap();
    fs::set_permissions(&regular, fs::Permissions::from_mode(0o644)).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged_script = ws.path().join("run.sh");
    let staged_regular = ws.path().join("data.txt");

    // fs::copy preserves permissions on Unix.
    let script_mode = fs::metadata(&staged_script).unwrap().permissions().mode() & 0o777;
    let data_mode = fs::metadata(&staged_regular).unwrap().permissions().mode() & 0o777;

    assert_eq!(
        script_mode, 0o755,
        "executable permission should be preserved"
    );
    assert_eq!(data_mode, 0o644, "regular permission should be preserved");
}

// ===========================================================================
// 5. Concurrent workspace creation (multiple stagings in parallel)
// ===========================================================================

#[test]
fn concurrent_staging_creates_isolated_workspaces() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "shared content").unwrap();

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let root = src.path().to_path_buf();
            std::thread::spawn(move || {
                let ws = WorkspaceStager::new()
                    .source_root(&root)
                    .with_git_init(false)
                    .stage()
                    .unwrap();

                // Each workspace is independent; mutate it.
                fs::write(ws.path().join("unique.txt"), format!("worker {i}")).unwrap();
                let content = fs::read_to_string(ws.path().join("shared.txt")).unwrap();
                assert_eq!(content, "shared content");
                let unique = fs::read_to_string(ws.path().join("unique.txt")).unwrap();
                (ws.path().to_path_buf(), unique)
            })
        })
        .collect();

    let mut paths = Vec::new();
    for handle in handles {
        let (path, unique) = handle.join().unwrap();
        // Each workspace lives at a distinct path.
        assert!(!paths.contains(&path), "duplicate workspace path detected");
        paths.push(path);
        assert!(unique.starts_with("worker "));
    }
    assert_eq!(paths.len(), 8);
}

#[test]
fn concurrent_staging_with_git_init() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let root = src.path().to_path_buf();
            std::thread::spawn(move || {
                let ws = WorkspaceManager::prepare(&WorkspaceSpec {
                    root: root.to_string_lossy().to_string(),
                    mode: WorkspaceMode::Staged,
                    include: vec![],
                    exclude: vec![],
                })
                .unwrap();

                assert!(ws.path().join(".git").exists());
                let log = git(ws.path(), &["log", "--format=%s"]);
                assert!(log.contains("baseline"));
                ws.path().to_path_buf()
            })
        })
        .collect();

    let mut all_paths = std::collections::HashSet::new();
    for h in handles {
        let p = h.join().unwrap();
        all_paths.insert(p);
    }
    assert_eq!(all_paths.len(), 4, "each concurrent staging must be unique");
}

// ===========================================================================
// 6. Workspace cleanup on error (ensure temp dirs are removed)
// ===========================================================================

#[test]
fn cleanup_on_drop_after_normal_use() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let staged_path;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .stage()
            .unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists());
        // Write extra files to make cleanup more interesting.
        fs::create_dir_all(staged_path.join("sub")).unwrap();
        fs::write(staged_path.join("sub").join("extra.txt"), "extra").unwrap();
    }
    assert!(
        !staged_path.exists(),
        "workspace must be cleaned up after drop even with extra files"
    );
}

#[test]
fn cleanup_happens_even_after_git_operations() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main(){}").unwrap();

    let staged_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();

        // Perform git operations that create additional objects.
        fs::write(ws.path().join("new_file.rs"), "fn new(){}").unwrap();
        git(ws.path(), &["add", "-A"]);
        git(
            ws.path(),
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@test",
                "commit",
                "-m",
                "second commit",
            ],
        );
    }
    assert!(
        !staged_path.exists(),
        "workspace must be removed even after git commits"
    );
}

#[test]
fn stager_error_does_not_leak_temp_dir() {
    // When stage() fails because source doesn't exist, no temp dir should leak.
    let result = WorkspaceStager::new()
        .source_root("/this/path/definitely/does/not/exist/anywhere")
        .stage();
    assert!(result.is_err());
    // We can't directly verify temp dir cleanup from a failed stage since the
    // TempDir was never returned, but the error path should not panic.
}

// ===========================================================================
// 7. Workspace diff generation (git diff between baseline and modified)
// ===========================================================================

#[test]
fn diff_summary_empty_for_unmodified_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary.is_empty(),
        "unmodified workspace should have no diff"
    );
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

#[test]
fn diff_summary_detects_added_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.txt"), "existing").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new_file.txt"), "new content\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary
            .added
            .iter()
            .any(|p| p.to_string_lossy().contains("new_file.txt")),
        "added files should include new_file.txt, got: {:?}",
        summary.added
    );
    assert!(summary.total_additions > 0);
}

#[test]
fn diff_summary_detects_modified_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "line1\nline2\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "line1\nmodified\nline3\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary
            .modified
            .iter()
            .any(|p| p.to_string_lossy().contains("data.txt")),
        "modified files should include data.txt"
    );
    assert!(summary.total_additions > 0);
    assert!(summary.total_deletions > 0);
}

#[test]
fn diff_summary_detects_deleted_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "goodbye\n").unwrap();
    fs::write(src.path().join("keep.txt"), "stay\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary
            .deleted
            .iter()
            .any(|p| p.to_string_lossy().contains("doomed.txt")),
        "deleted files should include doomed.txt"
    );
    assert!(summary.total_deletions > 0);
}

#[test]
fn diff_summary_counts_multiple_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a\n").unwrap();
    fs::write(src.path().join("b.txt"), "b\n").unwrap();
    fs::write(src.path().join("c.txt"), "c\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Add
    fs::write(ws.path().join("d.txt"), "d\n").unwrap();
    // Modify
    fs::write(ws.path().join("a.txt"), "a modified\n").unwrap();
    // Delete
    fs::remove_file(ws.path().join("b.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(
        summary.file_count(),
        3,
        "should have 3 changed files (added + modified + deleted)"
    );
}

#[test]
fn diff_summary_with_subdirectory_changes() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::write(src.path().join("src").join("main.rs"), "fn main(){}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::create_dir_all(ws.path().join("src").join("sub")).unwrap();
    fs::write(
        ws.path().join("src").join("sub").join("mod.rs"),
        "pub mod sub;",
    )
    .unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        !summary.added.is_empty(),
        "new file in subdirectory should be detected"
    );
}

// ===========================================================================
// 8. Custom glob patterns (complex include/exclude combinations)
// ===========================================================================

#[test]
fn glob_multiple_extensions_include() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f(){}").unwrap();
    fs::write(src.path().join("config.toml"), "[pkg]").unwrap();
    fs::write(src.path().join("data.json"), "{}").unwrap();
    fs::write(src.path().join("notes.md"), "# Notes").unwrap();
    fs::write(src.path().join("image.png"), "PNG").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.rs".into(), "*.toml".into(), "*.json".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(files.contains(&"config.toml".to_string()));
    assert!(files.contains(&"data.json".to_string()));
    assert!(!files.contains(&"notes.md".to_string()));
    assert!(!files.contains(&"image.png".to_string()));
}

#[test]
fn glob_nested_exclude_with_broad_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src").join("generated")).unwrap();
    fs::create_dir_all(src.path().join("src").join("core")).unwrap();
    fs::write(
        src.path().join("src").join("generated").join("out.rs"),
        "generated",
    )
    .unwrap();
    fs::write(
        src.path().join("src").join("core").join("lib.rs"),
        "pub fn core(){}",
    )
    .unwrap();
    fs::write(src.path().join("src").join("main.rs"), "fn main(){}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/generated/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("core/lib.rs")));
    assert!(files.iter().any(|f| f.contains("main.rs")));
    assert!(
        !files.iter().any(|f| f.contains("generated")),
        "generated files should be excluded"
    );
}

#[test]
fn glob_exclude_specific_filenames_across_tree() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a")).unwrap();
    fs::create_dir_all(src.path().join("b")).unwrap();
    fs::write(src.path().join("a").join("Thumbs.db"), "cache").unwrap();
    fs::write(src.path().join("b").join("Thumbs.db"), "cache").unwrap();
    fs::write(src.path().join("a").join("real.txt"), "real").unwrap();
    fs::write(src.path().join("Thumbs.db"), "root cache").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["**/Thumbs.db".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(
        !files.iter().any(|f| f.contains("Thumbs.db")),
        "Thumbs.db should be excluded everywhere"
    );
    assert!(files.iter().any(|f| f.contains("real.txt")));
}

#[test]
fn glob_overlapping_include_exclude() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("logs")).unwrap();
    fs::write(src.path().join("logs").join("app.log"), "log data").unwrap();
    fs::write(src.path().join("logs").join("important.log"), "important").unwrap();
    fs::write(src.path().join("app.rs"), "fn main(){}").unwrap();

    // Include everything in logs, but also exclude *.log — exclude wins.
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["logs/**".into(), "*.rs".into()],
        vec!["*.log".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(
        !files.iter().any(|f| f.ends_with(".log")),
        "exclude should override include for .log files"
    );
    assert!(files.contains(&"app.rs".to_string()));
}

#[test]
fn glob_with_brace_expansion() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("lib.rs"), "lib").unwrap();
    fs::write(src.path().join("test.rs"), "test").unwrap();
    fs::write(src.path().join("config.yaml"), "yaml").unwrap();
    fs::write(src.path().join("config.json"), "json").unwrap();
    fs::write(src.path().join("notes.md"), "md").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.{rs,yaml,json}".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"lib.rs".to_string()));
    assert!(files.contains(&"config.yaml".to_string()));
    assert!(files.contains(&"config.json".to_string()));
    assert!(!files.contains(&"notes.md".to_string()));
}

#[test]
fn glob_empty_include_with_multiple_excludes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f(){}").unwrap();
    fs::write(src.path().join("build.log"), "building...").unwrap();
    fs::write(src.path().join("temp.tmp"), "temp").unwrap();
    fs::create_dir_all(src.path().join("target")).unwrap();
    fs::write(src.path().join("target").join("output.o"), "obj").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["*.log".into(), "*.tmp".into(), "target/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(!files.iter().any(|f| f.ends_with(".log")));
    assert!(!files.iter().any(|f| f.ends_with(".tmp")));
    assert!(!files.iter().any(|f| f.starts_with("target")));
}

// ===========================================================================
// 9. Workspace re-staging (update existing staged workspace)
// ===========================================================================

#[test]
fn restage_captures_mutations_from_first_stage() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("base.txt"), "v1").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Mutate ws1
    fs::write(ws1.path().join("base.txt"), "v2").unwrap();
    fs::write(ws1.path().join("new.txt"), "added in ws1").unwrap();
    fs::create_dir_all(ws1.path().join("sub")).unwrap();
    fs::write(ws1.path().join("sub").join("nested.txt"), "nested").unwrap();

    // Re-stage from ws1
    let ws2 = WorkspaceManager::prepare(&staged_spec(ws1.path())).unwrap();

    assert_ne!(ws1.path(), ws2.path());
    assert_eq!(
        fs::read_to_string(ws2.path().join("base.txt")).unwrap(),
        "v2"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("new.txt")).unwrap(),
        "added in ws1"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("sub").join("nested.txt")).unwrap(),
        "nested"
    );
}

#[test]
fn restage_with_different_globs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f(){}").unwrap();
    fs::write(src.path().join("data.json"), "{}").unwrap();
    fs::write(src.path().join("notes.md"), "# Notes").unwrap();

    // First stage: include everything
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(collect_files(ws1.path()).len(), 3);

    // Re-stage from ws1 with narrower globs
    let ws2 =
        WorkspaceManager::prepare(&staged_spec_globs(ws1.path(), vec!["*.rs".into()], vec![]))
            .unwrap();

    let files2 = collect_files(ws2.path());
    assert_eq!(files2.len(), 1);
    assert!(files2.contains(&"code.rs".to_string()));
}

#[test]
fn restage_gets_clean_git_baseline() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Modify ws1 to have dirty state
    fs::write(ws1.path().join("file.txt"), "modified").unwrap();

    // Re-stage picks up modified content but starts clean
    let ws2 = WorkspaceManager::prepare(&staged_spec(ws1.path())).unwrap();

    let status = git(ws2.path(), &["status", "--porcelain=v1"]);
    assert!(
        status.is_empty(),
        "re-staged workspace should have clean git state"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("file.txt")).unwrap(),
        "modified"
    );
}

// ===========================================================================
// 10. Path traversal prevention (absolute paths, ../.. in patterns)
// ===========================================================================

#[test]
fn absolute_path_in_workspace_does_not_escape_staging() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("safe.txt"), "safe").unwrap();

    // The staging should operate normally; paths in the workspace are relative.
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert!(ws.path().join("safe.txt").exists());
    // The staged workspace must be contained in a temp directory.
    assert!(ws.path().starts_with(std::env::temp_dir()));
}

#[test]
fn dot_dot_in_exclude_pattern_does_not_break_staging() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "data").unwrap();

    // Patterns with .. should not cause staging to fail or access outside dirs.
    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec!["../**".into()]))
            .unwrap();

    // Files should still be staged normally since .. doesn't match relative paths.
    let files = collect_files(ws.path());
    assert!(files.contains(&"file.txt".to_string()));
}

#[test]
fn relative_paths_remain_relative_in_staged_workspace() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::write(src.path().join("a").join("b").join("deep.txt"), "deep").unwrap();
    fs::write(src.path().join("top.txt"), "top").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    // All paths should be relative with forward slashes.
    for f in &files {
        assert!(!f.starts_with('/'), "path should be relative: {f}");
        assert!(!f.contains(".."), "path should not contain ..: {f}");
    }
}

#[test]
fn stager_rejects_nonexistent_source() {
    let result = WorkspaceStager::new()
        .source_root("/absolutely/nonexistent/path")
        .stage();
    assert!(result.is_err());
}

// ===========================================================================
// Bonus: WorkspaceStager builder edge cases
// ===========================================================================

#[test]
fn stager_without_git_init_has_no_dot_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("f.txt").exists());
}

#[test]
fn stager_include_and_exclude_combined() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("tests")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "lib").unwrap();
    fs::write(src.path().join("src").join("secret.key"), "key").unwrap();
    fs::write(src.path().join("tests").join("test.rs"), "test").unwrap();
    fs::write(src.path().join("README.md"), "readme").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into(), "tests/**".into()])
        .exclude(vec!["**/*.key".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("lib.rs")));
    assert!(files.iter().any(|f| f.contains("test.rs")));
    assert!(!files.iter().any(|f| f.contains("secret.key")));
    assert!(!files.iter().any(|f| f.contains("README.md")));
}

#[test]
fn stager_default_is_equivalent_to_new() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::default()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join("f.txt").exists());
    assert!(ws.path().join(".git").exists());
}

// ===========================================================================
// Snapshot integration with staging
// ===========================================================================

#[test]
fn snapshot_capture_on_staged_workspace() {
    use abp_workspace::snapshot;

    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "aaa").unwrap();
    fs::write(src.path().join("b.txt"), "bbb").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let snap = snapshot::capture(ws.path()).unwrap();
    assert_eq!(snap.file_count(), 2);
    assert!(snap.has_file(std::path::Path::new("a.txt")));
    assert!(snap.has_file(std::path::Path::new("b.txt")));
    assert_eq!(snap.get_file("a.txt").unwrap().size, 3);
}

#[test]
fn snapshot_compare_before_and_after_mutation() {
    use abp_workspace::snapshot;

    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let before = snapshot::capture(ws.path()).unwrap();

    fs::write(ws.path().join("file.txt"), "modified").unwrap();
    fs::write(ws.path().join("new.txt"), "new").unwrap();

    let after = snapshot::capture(ws.path()).unwrap();
    let diff = snapshot::compare(&before, &after);

    assert_eq!(diff.added.len(), 1);
    assert_eq!(diff.modified.len(), 1);
    assert!(diff.removed.is_empty());
}

// ===========================================================================
// 11. StagedWorkspace creation basics
// ===========================================================================

#[test]
fn prepared_workspace_path_is_valid_directory() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(
        ws.path().is_dir(),
        "prepared workspace path must be a directory"
    );
}

#[test]
fn prepared_workspace_path_is_absolute() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().is_absolute(), "workspace path must be absolute");
}

#[test]
fn passthrough_mode_uses_original_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

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
fn staged_mode_uses_different_path_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path(), "staged path must differ from source");
}

#[test]
fn staged_workspace_contains_source_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "world").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("hello.txt")).unwrap(),
        "world"
    );
}

// ===========================================================================
// 12. File copying with include patterns
// ===========================================================================

#[test]
fn include_single_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "rust").unwrap();
    fs::write(src.path().join("b.py"), "python").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]))
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
    assert!(!files.contains(&"b.py".to_string()));
}

#[test]
fn include_directory_pattern() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("docs")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "lib").unwrap();
    fs::write(src.path().join("docs").join("readme.md"), "docs").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("lib.rs")));
    assert!(!files.iter().any(|f| f.contains("readme.md")));
}

#[test]
fn include_wildcard_in_subdirectory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a")).unwrap();
    fs::create_dir_all(src.path().join("b")).unwrap();
    fs::write(src.path().join("a").join("x.rs"), "x").unwrap();
    fs::write(src.path().join("b").join("y.rs"), "y").unwrap();
    fs::write(src.path().join("root.rs"), "root").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec!["a/**".into()], vec![]))
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("x.rs")));
    assert!(!files.iter().any(|f| f.contains("y.rs")));
    assert!(!files.contains(&"root.rs".to_string()));
}

#[test]
fn include_empty_means_include_all() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.rs"), "b").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec![])).unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files.len(), 2);
}

#[test]
fn include_deep_nested_glob() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    fs::write(
        src.path().join("a").join("b").join("c").join("deep.txt"),
        "deep",
    )
    .unwrap();
    fs::write(src.path().join("top.txt"), "top").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/deep.txt".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.ends_with("deep.txt")));
    assert!(!files.contains(&"top.txt".to_string()));
}

// ===========================================================================
// 13. File copying with exclude patterns
// ===========================================================================

#[test]
fn exclude_single_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f(){}").unwrap();
    fs::write(src.path().join("build.log"), "log").unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec!["*.log".into()]))
            .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(!files.contains(&"build.log".to_string()));
}

#[test]
fn exclude_directory_recursively() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("target").join("debug")).unwrap();
    fs::write(src.path().join("target").join("debug").join("bin"), "elf").unwrap();
    fs::write(src.path().join("Cargo.toml"), "[package]").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["target/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("target")));
    assert!(files.contains(&"Cargo.toml".to_string()));
}

#[test]
fn exclude_multiple_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f(){}").unwrap();
    fs::write(src.path().join("a.log"), "log1").unwrap();
    fs::write(src.path().join("b.tmp"), "tmp1").unwrap();
    fs::write(src.path().join("c.bak"), "bak1").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["*.log".into(), "*.tmp".into(), "*.bak".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files, vec!["code.rs"]);
}

#[test]
fn exclude_hidden_files_with_glob() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".secret"), "s").unwrap();
    fs::write(src.path().join("public.txt"), "p").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec![".*".into()]))
        .unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files, vec!["public.txt"]);
}

// ===========================================================================
// 14. Include + exclude combined
// ===========================================================================

#[test]
fn include_rs_exclude_test_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("lib.rs"), "lib").unwrap();
    fs::write(src.path().join("main.rs"), "main").unwrap();
    fs::write(src.path().join("test_lib.rs"), "test").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.rs".into()],
        vec!["test_*".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"lib.rs".to_string()));
    assert!(files.contains(&"main.rs".to_string()));
    assert!(!files.contains(&"test_lib.rs".to_string()));
}

#[test]
fn include_src_exclude_generated_within_src() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src").join("gen")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "lib").unwrap();
    fs::write(src.path().join("src").join("gen").join("auto.rs"), "auto").unwrap();
    fs::write(src.path().join("readme.md"), "readme").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec!["src/gen/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().any(|f| f.contains("lib.rs")));
    assert!(!files.iter().any(|f| f.contains("auto.rs")));
    assert!(!files.iter().any(|f| f.contains("readme.md")));
}

#[test]
fn exclude_takes_precedence_over_include() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("wanted.rs"), "yes").unwrap();
    fs::write(src.path().join("unwanted.rs"), "no").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.rs".into()],
        vec!["unwanted.*".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"wanted.rs".to_string()));
    assert!(!files.contains(&"unwanted.rs".to_string()));
}

// ===========================================================================
// 15. .git directory exclusion
// ===========================================================================

#[test]
fn source_dot_git_is_never_copied() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join(".git").join("objects").join("pack"), "data").unwrap();
    fs::write(src.path().join("code.rs"), "fn f(){}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn nested_dot_git_excluded() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("vendor").join(".git")).unwrap();
    fs::write(
        src.path().join("vendor").join(".git").join("config"),
        "git config",
    )
    .unwrap();
    fs::write(src.path().join("vendor").join("lib.rs"), "lib").unwrap();

    // Note: WalkDir filter_entry on ".git" will skip any directory named .git
    // at any depth.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("vendor").join("lib.rs").exists());
    assert!(!ws.path().join("vendor").join(".git").exists());
}

#[test]
fn dot_git_excluded_even_with_include_all_glob() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref").unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec!["**".into()], vec![]))
        .unwrap();

    let all = collect_files(ws.path());
    assert!(!all.iter().any(|f| f.contains(".git")));
}

// ===========================================================================
// 16. Git repo initialization
// ===========================================================================

#[test]
fn staged_workspace_has_git_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn git_repo_is_valid() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let rev = git(ws.path(), &["rev-parse", "--is-inside-work-tree"]);
    assert_eq!(rev, "true");
}

#[test]
fn stager_with_git_init_true_creates_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_with_git_init_false_skips_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
}

// ===========================================================================
// 17. Baseline commit creation
// ===========================================================================

#[test]
fn baseline_commit_exists_after_staging() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(!log.is_empty(), "should have at least one commit");
}

#[test]
fn baseline_commit_message_contains_baseline() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--format=%s"]);
    assert!(
        log.to_lowercase().contains("baseline"),
        "commit message should contain 'baseline', got: {log}"
    );
}

#[test]
fn baseline_commit_includes_all_staged_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("c.txt"), "c").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ls = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(ls.contains("a.txt"));
    assert!(ls.contains("b.txt"));
    assert!(ls.contains("c.txt"));
}

#[test]
fn workspace_is_clean_after_baseline() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(
        status.is_empty(),
        "workspace should be clean after baseline commit"
    );
}

// ===========================================================================
// 18. Diff computation
// ===========================================================================

#[test]
fn diff_workspace_returns_sorted_paths() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("c.txt"), "c").unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("z_new.txt"), "z\n").unwrap();
    fs::write(ws.path().join("a_new.txt"), "a\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    let added_strs: Vec<String> = summary
        .added
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let mut sorted = added_strs.clone();
    sorted.sort();
    assert_eq!(added_strs, sorted, "added paths should be sorted");
}

#[test]
fn diff_total_additions_matches_line_count() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "base\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new.txt"), "line1\nline2\nline3\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(
        summary.total_additions >= 3,
        "should count at least 3 added lines"
    );
}

#[test]
fn diff_empty_workspace_is_empty() {
    let src = tempdir().unwrap();
    // No files at all — just the directory.
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

#[test]
fn diff_summary_file_count_correct() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a\n").unwrap();
    fs::write(src.path().join("b.txt"), "b\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("c.txt"), "c\n").unwrap();
    fs::write(ws.path().join("d.txt"), "d\n").unwrap();
    fs::write(ws.path().join("a.txt"), "a modified\n").unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    // 2 added + 1 modified + 1 deleted = 4
    assert_eq!(summary.file_count(), 4);
}

#[test]
fn diff_summary_total_changes_is_additions_plus_deletions() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "replaced\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(
        summary.total_changes(),
        summary.total_additions + summary.total_deletions,
    );
}

// ===========================================================================
// 19. Deep directory trees
// ===========================================================================

#[test]
fn very_deep_nesting_15_levels() {
    let src = tempdir().unwrap();
    let mut dir = src.path().to_path_buf();
    for i in 0..15 {
        dir = dir.join(format!("d{i}"));
    }
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("leaf.txt"), "leaf").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(
        files.iter().any(|f| f.ends_with("leaf.txt")),
        "deeply nested file should be staged"
    );
}

#[test]
fn mixed_depths_files_all_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "r").unwrap();
    fs::create_dir_all(src.path().join("a")).unwrap();
    fs::write(src.path().join("a").join("mid.txt"), "m").unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();
    fs::write(
        src.path().join("a").join("b").join("c").join("deep.txt"),
        "d",
    )
    .unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("root.txt").exists());
    assert!(ws.path().join("a").join("mid.txt").exists());
    assert!(
        ws.path()
            .join("a")
            .join("b")
            .join("c")
            .join("deep.txt")
            .exists()
    );
}

// ===========================================================================
// 20. Symlink handling (additional cases)
// ===========================================================================

#[test]
fn regular_file_alongside_symlink_is_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();
    fs::write(src.path().join("also_real.txt"), "also real").unwrap();

    // Even if symlink creation fails, regular files must be staged.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("real.txt").exists());
    assert!(ws.path().join("also_real.txt").exists());
}

// ===========================================================================
// 21. Empty directory handling
// ===========================================================================

#[test]
fn empty_source_directory_produces_empty_workspace() {
    let src = tempdir().unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn empty_subdirectories_are_created() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_dir")).unwrap();
    fs::write(src.path().join("file.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The empty dir may or may not be created (WalkDir yields it, and
    // copy_workspace calls create_dir_all for dirs, so it should exist).
    assert!(ws.path().join("file.txt").exists());
    assert!(
        ws.path().join("empty_dir").exists(),
        "empty subdirectory should be created"
    );
}

#[test]
fn multiple_empty_subdirectories() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a")).unwrap();
    fs::create_dir_all(src.path().join("b")).unwrap();
    fs::create_dir_all(src.path().join("c")).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("a").is_dir());
    assert!(ws.path().join("b").is_dir());
    assert!(ws.path().join("c").is_dir());
}

// ===========================================================================
// 22. Large file handling
// ===========================================================================

#[test]
fn large_file_is_copied_correctly() {
    let src = tempdir().unwrap();
    let large_content: String = "x".repeat(1_000_000); // 1MB
    fs::write(src.path().join("large.bin"), &large_content).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = fs::read_to_string(ws.path().join("large.bin")).unwrap();
    assert_eq!(staged.len(), 1_000_000);
    assert_eq!(staged, large_content);
}

#[test]
fn binary_file_is_copied_correctly() {
    let src = tempdir().unwrap();
    let binary_data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    fs::write(src.path().join("data.bin"), &binary_data).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = fs::read(ws.path().join("data.bin")).unwrap();
    assert_eq!(staged, binary_data);
}

#[test]
fn zero_byte_file_is_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("empty.txt").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

// ===========================================================================
// 23. Unicode path handling
// ===========================================================================

#[test]
fn unicode_filename_is_staged() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("café.txt"), "coffee").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("café.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("café.txt")).unwrap(),
        "coffee"
    );
}

#[test]
fn unicode_directory_name_is_staged() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("données")).unwrap();
    fs::write(src.path().join("données").join("info.txt"), "info").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("données").join("info.txt").exists());
}

#[test]
fn unicode_content_preserved() {
    let src = tempdir().unwrap();
    let content = "日本語テスト 🎉 Ñoño";
    fs::write(src.path().join("unicode.txt"), content).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("unicode.txt")).unwrap(),
        content
    );
}

#[test]
fn emoji_filename() {
    let src = tempdir().unwrap();
    // Some filesystems support emoji in filenames.
    if fs::write(src.path().join("🚀.txt"), "rocket").is_err() {
        return; // Filesystem doesn't support this.
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    if ws.path().join("🚀.txt").exists() {
        assert_eq!(
            fs::read_to_string(ws.path().join("🚀.txt")).unwrap(),
            "rocket"
        );
    }
}

// ===========================================================================
// 24. Workspace cleanup
// ===========================================================================

#[test]
fn workspace_cleaned_up_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let path;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();
        path = ws.path().to_path_buf();
        assert!(path.exists());
    }
    assert!(!path.exists(), "workspace should be cleaned up on drop");
}

#[test]
fn multiple_workspaces_cleanup_independently() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let p1 = ws1.path().to_path_buf();
    let p2 = ws2.path().to_path_buf();
    assert_ne!(p1, p2);

    drop(ws1);
    assert!(!p1.exists(), "ws1 should be cleaned up");
    assert!(p2.exists(), "ws2 should still exist");

    drop(ws2);
    assert!(!p2.exists(), "ws2 should be cleaned up");
}

#[test]
fn workspace_with_readonly_files_cleanup() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let path = ws.path().to_path_buf();
    assert!(path.exists());

    // Make a file read-only.
    let fp = ws.path().join("f.txt");
    let mut perms = fs::metadata(&fp).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&fp, perms).unwrap();

    // Drop the workspace; TempDir cleanup might fail on read-only files
    // on some systems — we just verify no panic occurred.
    drop(ws);
}

// ===========================================================================
// 25. Concurrent workspace creation
// ===========================================================================

#[test]
fn concurrent_stager_workspaces_are_isolated() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "shared").unwrap();

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let root = src.path().to_path_buf();
            std::thread::spawn(move || {
                let ws = WorkspaceStager::new()
                    .source_root(&root)
                    .with_git_init(false)
                    .stage()
                    .unwrap();
                // Mutate each workspace independently.
                fs::write(ws.path().join("local.txt"), format!("worker-{i}")).unwrap();
                let local = fs::read_to_string(ws.path().join("local.txt")).unwrap();
                assert_eq!(local, format!("worker-{i}"));
                ws.path().to_path_buf()
            })
        })
        .collect();

    let mut paths = std::collections::HashSet::new();
    for h in handles {
        paths.insert(h.join().unwrap());
    }
    assert_eq!(paths.len(), 4, "all workspaces at distinct paths");
}

#[test]
fn concurrent_staging_with_different_globs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "rust").unwrap();
    fs::write(src.path().join("b.py"), "python").unwrap();
    fs::write(src.path().join("c.js"), "javascript").unwrap();

    let handles: Vec<_> = vec![("*.rs", 1usize), ("*.py", 1), ("*.js", 1)]
        .into_iter()
        .map(|(pattern, expected)| {
            let root = src.path().to_path_buf();
            let pat = pattern.to_string();
            std::thread::spawn(move || {
                let ws = WorkspaceManager::prepare(&staged_spec_globs(&root, vec![pat], vec![]))
                    .unwrap();
                let files = collect_files(ws.path());
                assert_eq!(files.len(), expected);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 26. Workspace configuration options
// ===========================================================================

#[test]
fn stager_source_root_required() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err(), "stage without source_root should fail");
}

#[test]
fn stager_builder_chain_fluent() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    // Verify the fluent API compiles and works end-to-end.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.txt".into()])
        .exclude(vec!["*.log".into()])
        .with_git_init(true)
        .stage()
        .unwrap();

    assert!(ws.path().join("f.txt").exists());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_override_include_replaces_previous() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "rs").unwrap();
    fs::write(src.path().join("b.py"), "py").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.py".into()])
        .include(vec!["*.rs".into()]) // Second call replaces the first.
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
    assert!(!files.contains(&"b.py".to_string()));
}

#[test]
fn stager_override_exclude_replaces_previous() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "rs").unwrap();
    fs::write(src.path().join("b.log"), "log").unwrap();
    fs::write(src.path().join("c.tmp"), "tmp").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.rs".into()])
        .exclude(vec!["*.log".into()]) // Second call replaces the first.
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    // *.rs should no longer be excluded since the second call replaced.
    assert!(files.contains(&"a.rs".to_string()));
    assert!(!files.contains(&"b.log".to_string()));
    assert!(files.contains(&"c.tmp".to_string()));
}

// ===========================================================================
// 27. Staged workspace path validation
// ===========================================================================

#[test]
fn staged_path_is_under_temp_directory() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(
        ws.path().starts_with(std::env::temp_dir()),
        "staged workspace should be under system temp dir"
    );
}

#[test]
fn staged_path_differs_each_call() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws1.path(), ws2.path());
}

#[test]
fn staged_path_contains_no_source_components() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The staged path should be a temp dir, not contain the source path.
    assert!(!ws.path().starts_with(src.path()));
}

// ===========================================================================
// 28. WorkspaceManager git_status / git_diff
// ===========================================================================

#[test]
fn git_status_on_clean_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.is_some(),
        "git_status should succeed on staged workspace"
    );
    assert!(
        status.unwrap().is_empty(),
        "clean workspace should have empty status"
    );
}

#[test]
fn git_status_shows_modified_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "modified\n").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(!status.is_empty(), "modified file should show in status");
    assert!(status.contains("f.txt"));
}

#[test]
fn git_diff_on_clean_workspace_is_empty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    assert!(diff.unwrap().is_empty());
}

#[test]
fn git_diff_shows_modifications() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "changed\n").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(!diff.is_empty());
    assert!(diff.contains("changed"));
}

#[test]
fn git_status_returns_none_for_non_git_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.is_none(),
        "git_status on non-git dir should return None"
    );
}

// ===========================================================================
// 29. File content fidelity
// ===========================================================================

#[test]
fn file_content_preserved_exactly() {
    let src = tempdir().unwrap();
    let content = "line1\r\nline2\nline3\ttab\0null";
    fs::write(src.path().join("mixed.bin"), content.as_bytes()).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = fs::read(ws.path().join("mixed.bin")).unwrap();
    assert_eq!(staged, content.as_bytes());
}

#[test]
fn many_small_files_content_fidelity() {
    let src = tempdir().unwrap();
    for i in 0..100 {
        fs::write(
            src.path().join(format!("f{i:03}.txt")),
            format!("content-{i}"),
        )
        .unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    for i in 0..100 {
        let content = fs::read_to_string(ws.path().join(format!("f{i:03}.txt"))).unwrap();
        assert_eq!(content, format!("content-{i}"));
    }
}

// ===========================================================================
// 30. Snapshot module integration
// ===========================================================================

#[test]
fn snapshot_file_count_matches_collect_files() {
    use abp_workspace::snapshot;

    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("c.txt"), "c").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let snap = snapshot::capture(ws.path()).unwrap();
    let files = collect_files(ws.path());
    assert_eq!(snap.file_count(), files.len());
}

#[test]
fn snapshot_total_size_is_sum_of_file_sizes() {
    use abp_workspace::snapshot;

    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "aaaa").unwrap(); // 4 bytes
    fs::write(src.path().join("b.txt"), "bb").unwrap(); // 2 bytes

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let snap = snapshot::capture(ws.path()).unwrap();
    assert_eq!(snap.total_size(), 6);
}

#[test]
fn snapshot_compare_identical_is_all_unchanged() {
    use abp_workspace::snapshot;

    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let s1 = snapshot::capture(ws.path()).unwrap();
    let s2 = snapshot::capture(ws.path()).unwrap();
    let diff = snapshot::compare(&s1, &s2);

    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 1);
}

#[test]
fn snapshot_detects_removal() {
    use abp_workspace::snapshot;

    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let before = snapshot::capture(ws.path()).unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();
    let after = snapshot::capture(ws.path()).unwrap();

    let diff = snapshot::compare(&before, &after);
    assert_eq!(diff.removed.len(), 1);
    assert!(diff.removed[0].to_string_lossy().contains("b.txt"));
}

// ===========================================================================
// 31. Edge cases
// ===========================================================================

#[test]
fn filename_with_spaces() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("my file.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("my file.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("my file.txt")).unwrap(),
        "data"
    );
}

#[test]
fn filename_with_special_characters() {
    let src = tempdir().unwrap();
    // Characters that are valid on most filesystems.
    fs::write(src.path().join("file-name_v2 (1).txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("file-name_v2 (1).txt").exists());
}

#[test]
fn very_long_filename() {
    let src = tempdir().unwrap();
    // Most filesystems support up to 255 chars.
    let long_name = format!("{}.txt", "a".repeat(200));
    if fs::write(src.path().join(&long_name), "data").is_err() {
        return; // Skip if filesystem doesn't support.
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(&long_name).exists());
}

#[test]
fn file_and_directory_with_same_prefix() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("data")).unwrap();
    fs::write(src.path().join("data").join("inner.txt"), "inner").unwrap();
    fs::write(src.path().join("data.txt"), "flat").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("data").join("inner.txt").exists());
    assert!(ws.path().join("data.txt").exists());
}

#[test]
fn glob_question_mark_wildcard() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a1.txt"), "1").unwrap();
    fs::write(src.path().join("a2.txt"), "2").unwrap();
    fs::write(src.path().join("ab.txt"), "b").unwrap();
    fs::write(src.path().join("xyz.txt"), "x").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["a?.txt".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"a1.txt".to_string()));
    assert!(files.contains(&"a2.txt".to_string()));
    assert!(files.contains(&"ab.txt".to_string()));
    assert!(!files.contains(&"xyz.txt".to_string()));
}

#[test]
fn passthrough_mode_does_not_create_temp_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // Passthrough should reference the original, not create a copy.
    assert_eq!(ws.path(), src.path());
    assert!(ws.path().join("f.txt").exists());
}

#[test]
fn staging_preserves_nested_directory_structure() {
    let src = tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "leaf").unwrap();
    fs::write(src.path().join("a").join("mid.txt"), "mid").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        ws.path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("leaf.txt")
            .exists()
    );
    assert!(ws.path().join("a").join("mid.txt").exists());
}

#[test]
fn staging_source_with_only_directories_no_files() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::create_dir_all(src.path().join("c")).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(
        files.is_empty(),
        "no files should be staged from directory-only source"
    );
}

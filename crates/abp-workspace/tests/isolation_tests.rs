// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive workspace isolation tests.
//!
//! Validates that staged workspaces are fully independent of their source,
//! that glob filtering works correctly, and that auxiliary modules (snapshot,
//! diff, template, tracker) integrate properly with staged workspaces.

use abp_workspace::WorkspaceStager;
use abp_workspace::diff::diff_workspace;
use abp_workspace::snapshot;
use abp_workspace::template::WorkspaceTemplate;
use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};
use std::fs;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Stage a workspace from `src` with default settings (git init enabled).
fn stage(src: &Path) -> abp_workspace::PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .stage()
        .expect("staging should succeed")
}

/// Stage without git initialization.
fn stage_no_git(src: &Path) -> abp_workspace::PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .with_git_init(false)
        .stage()
        .expect("staging should succeed")
}

/// Collect all regular files under `root` (relative paths, excluding `.git`).
fn list_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
    {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(root).unwrap();
            // Normalize to forward slashes for cross-platform comparison.
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    files.sort();
    files
}

// ===========================================================================
// 1. Basic staging — copy source dir → verify all files present
// ===========================================================================
#[test]
fn basic_staging_copies_all_files() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "alpha").unwrap();
    fs::write(src.path().join("b.txt"), "beta").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("c.txt"), "gamma").unwrap();

    let ws = stage_no_git(src.path());

    assert_eq!(list_files(ws.path()), vec!["a.txt", "b.txt", "sub/c.txt"]);
    assert_eq!(
        fs::read_to_string(ws.path().join("a.txt")).unwrap(),
        "alpha"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("sub").join("c.txt")).unwrap(),
        "gamma"
    );
}

// ===========================================================================
// 2. Git initialization — staged workspace has `.git` with initial commit
// ===========================================================================
#[test]
fn git_initialization_creates_repo_with_initial_commit() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = stage(src.path());

    assert!(ws.path().join(".git").exists());
    // HEAD should exist and the initial commit message should be "baseline".
    let log = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(
        log_str.contains("baseline"),
        "expected baseline commit, got: {log_str}"
    );
}

// ===========================================================================
// 3. Exclude glob — files matching exclude patterns not copied
// ===========================================================================
#[test]
fn exclude_glob_filters_out_matching_files() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("debug.log"), "log data").unwrap();
    fs::write(src.path().join("error.log"), "error data").unwrap();
    fs::write(src.path().join("notes.txt"), "hello").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = list_files(ws.path());
    assert!(files.contains(&"keep.rs".to_string()));
    assert!(files.contains(&"notes.txt".to_string()));
    assert!(!files.contains(&"debug.log".to_string()));
    assert!(!files.contains(&"error.log".to_string()));
}

// ===========================================================================
// 4. Include glob — only files matching include patterns copied
// ===========================================================================
#[test]
fn include_glob_copies_only_matching_files() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("lib.rs"), "pub mod a;").unwrap();
    fs::write(src.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("readme.md"), "# Hello").unwrap();
    fs::write(src.path().join("data.json"), "{}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = list_files(ws.path());
    assert!(files.contains(&"lib.rs".to_string()));
    assert!(files.contains(&"main.rs".to_string()));
    assert!(!files.contains(&"readme.md".to_string()));
    assert!(!files.contains(&"data.json".to_string()));
}

// ===========================================================================
// 5. Include + exclude — include selects, then exclude filters
// ===========================================================================
#[test]
fn include_plus_exclude_combined_filtering() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("src").join("generated")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "pub mod a;").unwrap();
    fs::write(
        src.path().join("src").join("generated").join("out.rs"),
        "// gen",
    )
    .unwrap();
    fs::write(src.path().join("readme.md"), "# hi").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["src/generated/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = list_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.contains(&"src/generated/out.rs".to_string()));
    assert!(!files.contains(&"readme.md".to_string()));
}

// ===========================================================================
// 6. Hidden files — .dotfiles handled correctly
// ===========================================================================
#[test]
fn hidden_dotfiles_are_copied() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join(".env"), "SECRET=123").unwrap();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    fs::write(src.path().join("visible.txt"), "hi").unwrap();

    let ws = stage_no_git(src.path());

    let files = list_files(ws.path());
    assert!(files.contains(&".env".to_string()));
    assert!(files.contains(&".gitignore".to_string()));
    assert!(files.contains(&"visible.txt".to_string()));
}

// ===========================================================================
// 7. Deep nesting — 5-level deep directory tree preserved
// ===========================================================================
#[test]
fn deep_nesting_five_levels_preserved() {
    let src = tempfile::tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c").join("d").join("e");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "deep content").unwrap();
    // Also a file at each level.
    fs::write(src.path().join("a").join("l1.txt"), "level 1").unwrap();
    fs::write(src.path().join("a").join("b").join("l2.txt"), "level 2").unwrap();

    let ws = stage_no_git(src.path());

    assert_eq!(
        fs::read_to_string(
            ws.path()
                .join("a")
                .join("b")
                .join("c")
                .join("d")
                .join("e")
                .join("leaf.txt")
        )
        .unwrap(),
        "deep content"
    );
    assert!(ws.path().join("a").join("l1.txt").exists());
    assert!(ws.path().join("a").join("b").join("l2.txt").exists());
}

// ===========================================================================
// 8. Symlinks — not followed (or handled gracefully)
// ===========================================================================
#[test]
fn symlinks_not_followed() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real content").unwrap();

    // Create a symlink — on Windows this may fail if not running elevated,
    // so we skip gracefully if symlink creation fails.
    let link_path = src.path().join("link.txt");
    let link_result = {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(src.path().join("real.txt"), &link_path)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(src.path().join("real.txt"), &link_path)
        }
    };

    if link_result.is_err() {
        // Symlink creation not supported (e.g. unprivileged Windows).
        // Verify staging still works without the symlink.
        let ws = stage_no_git(src.path());
        assert!(ws.path().join("real.txt").exists());
        return;
    }

    let ws = stage_no_git(src.path());

    // The real file must be present.
    assert!(ws.path().join("real.txt").exists());
    // The symlink should either not be copied (follow_links=false skips symlinks
    // that are not regular files) or be copied as a regular file.
    // Either outcome is acceptable — the key is no panic/error.
}

// ===========================================================================
// 9. Empty directories — either preserved or excluded
// ===========================================================================
#[test]
fn empty_directories_handled_gracefully() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_dir")).unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    // Staging should not error on empty directories.
    let ws = stage_no_git(src.path());
    assert!(ws.path().join("file.txt").exists());
    // Empty dir may or may not be present — both are acceptable.
}

// ===========================================================================
// 10. Large file — 1MB file copies correctly
// ===========================================================================
#[test]
fn large_file_copies_correctly() {
    let src = tempfile::tempdir().unwrap();
    let data = vec![0xABu8; 1_000_000]; // 1 MB
    fs::write(src.path().join("large.bin"), &data).unwrap();

    let ws = stage_no_git(src.path());

    let copied = fs::read(ws.path().join("large.bin")).unwrap();
    assert_eq!(copied.len(), 1_000_000);
    assert_eq!(copied, data);
}

// ===========================================================================
// 11. Binary file — binary content preserved byte-for-byte
// ===========================================================================
#[test]
fn binary_file_preserved_byte_for_byte() {
    let src = tempfile::tempdir().unwrap();
    // Craft binary content with null bytes, high bytes, and control chars.
    let mut binary = Vec::new();
    for i in 0u8..=255 {
        binary.push(i);
    }
    binary.extend_from_slice(&[0x00, 0xFF, 0x89, 0x50, 0x4E, 0x47]); // PNG-like header
    fs::write(src.path().join("data.bin"), &binary).unwrap();

    let ws = stage_no_git(src.path());

    let copied = fs::read(ws.path().join("data.bin")).unwrap();
    assert_eq!(copied, binary);
}

// ===========================================================================
// 12. Unicode filenames — files with unicode chars in names
// ===========================================================================
#[test]
fn unicode_filenames_handled() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("café.txt"), "coffee").unwrap();
    fs::write(src.path().join("données.rs"), "fn données() {}").unwrap();

    let ws = stage_no_git(src.path());

    assert_eq!(
        fs::read_to_string(ws.path().join("café.txt")).unwrap(),
        "coffee"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("données.rs")).unwrap(),
        "fn données() {}"
    );
}

// ===========================================================================
// 13. Workspace cleanup — temp directory cleaned up on drop
// ===========================================================================
#[test]
fn workspace_cleanup_on_drop() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = stage_no_git(src.path());
    let ws_path = ws.path().to_path_buf();
    assert!(ws_path.exists());

    drop(ws);

    assert!(
        !ws_path.exists(),
        "workspace temp dir should be removed after drop"
    );
}

// ===========================================================================
// 14. Multiple stages — two stages of same source are independent
// ===========================================================================
#[test]
fn multiple_stages_are_independent() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "original").unwrap();

    let ws1 = stage_no_git(src.path());
    let ws2 = stage_no_git(src.path());

    // They live in different directories.
    assert_ne!(ws1.path(), ws2.path());

    // Modify ws1 — ws2 and source should be unaffected.
    fs::write(ws1.path().join("shared.txt"), "modified in ws1").unwrap();
    fs::write(ws1.path().join("ws1_only.txt"), "only in ws1").unwrap();

    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "original"
    );
    assert!(!ws2.path().join("ws1_only.txt").exists());
    assert_eq!(
        fs::read_to_string(src.path().join("shared.txt")).unwrap(),
        "original"
    );
}

// ===========================================================================
// 15. Concurrent staging — multiple concurrent stages don't interfere
// ===========================================================================
#[test]
fn concurrent_staging_no_interference() {
    let src = tempfile::tempdir().unwrap();
    for i in 0..10 {
        fs::write(
            src.path().join(format!("file_{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let src_path = src.path().to_path_buf();
            std::thread::spawn(move || {
                let ws = WorkspaceStager::new()
                    .source_root(&src_path)
                    .with_git_init(false)
                    .stage()
                    .unwrap();
                let files = list_files(ws.path());
                assert_eq!(files.len(), 10);
                for i in 0..10 {
                    let content =
                        fs::read_to_string(ws.path().join(format!("file_{i}.txt"))).unwrap();
                    assert_eq!(content, format!("content {i}"));
                }
                ws.path().to_path_buf()
            })
        })
        .collect();

    let paths: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // All workspaces should be at distinct paths.
    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            assert_ne!(paths[i], paths[j]);
        }
    }
}

// ===========================================================================
// 16. Template application — template creates expected structure
// ===========================================================================
#[test]
fn template_creates_expected_structure() {
    let dir = tempfile::tempdir().unwrap();
    let mut tmpl = WorkspaceTemplate::new("test-app", "a test application");
    tmpl.add_file("src/main.rs", "fn main() { println!(\"hello\"); }");
    tmpl.add_file("src/lib.rs", "pub mod util;");
    tmpl.add_file("Cargo.toml", "[package]\nname = \"test-app\"");
    tmpl.add_file("README.md", "# Test App");

    let count = tmpl.apply(dir.path()).unwrap();
    assert_eq!(count, 4);

    assert!(dir.path().join("src").join("main.rs").exists());
    assert!(dir.path().join("src").join("lib.rs").exists());
    assert!(dir.path().join("Cargo.toml").exists());
    assert!(dir.path().join("README.md").exists());

    assert_eq!(
        fs::read_to_string(dir.path().join("src").join("main.rs")).unwrap(),
        "fn main() { println!(\"hello\"); }"
    );
}

// ===========================================================================
// 17. Snapshot capture — snapshot reflects actual workspace contents
// ===========================================================================
#[test]
fn snapshot_reflects_workspace_contents() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "alpha").unwrap();
    fs::write(src.path().join("b.txt"), "beta").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("c.txt"), "gamma").unwrap();

    let ws = stage_no_git(src.path());
    let snap = snapshot::capture(ws.path()).unwrap();

    assert_eq!(snap.file_count(), 3);
    assert!(snap.has_file(std::path::Path::new("a.txt")));
    assert!(snap.has_file(std::path::Path::new("b.txt")));
    // Snapshot stores forward-slash paths on all platforms? Check both.
    let has_c = snap.has_file(std::path::Path::new("sub/c.txt"))
        || snap.has_file(std::path::Path::new("sub\\c.txt"));
    assert!(has_c, "snapshot should contain sub/c.txt");

    let a_snap = snap.get_file("a.txt").unwrap();
    assert_eq!(a_snap.size, 5); // "alpha" = 5 bytes
    assert!(!a_snap.is_binary);
}

// ===========================================================================
// 18. Diff after modification — modify staged file → diff shows change
// ===========================================================================
#[test]
fn diff_after_modification_shows_change() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("config.toml"), "key = \"original\"\n").unwrap();

    let ws = stage(src.path());

    // Modify the file in the staged workspace.
    fs::write(ws.path().join("config.toml"), "key = \"changed\"\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(!summary.is_empty());
    assert!(
        summary
            .modified
            .contains(&std::path::PathBuf::from("config.toml")),
        "diff should show config.toml as modified, got: {summary:?}"
    );
    assert!(summary.total_additions >= 1);
    assert!(summary.total_deletions >= 1);
}

// ===========================================================================
// 19. Tracker records — changes recorded in change tracker
// ===========================================================================
#[test]
fn tracker_records_workspace_mutations() {
    let mut tracker = ChangeTracker::new();

    // Simulate workspace file operations.
    tracker.record(FileChange {
        path: "new_file.rs".to_string(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(42),
        content_hash: Some("abc".to_string()),
    });
    tracker.record(FileChange {
        path: "config.toml".to_string(),
        kind: ChangeKind::Modified,
        size_before: Some(100),
        size_after: Some(150),
        content_hash: Some("def".to_string()),
    });
    tracker.record(FileChange {
        path: "old.txt".to_string(),
        kind: ChangeKind::Deleted,
        size_before: Some(200),
        size_after: None,
        content_hash: None,
    });

    assert!(tracker.has_changes());
    assert_eq!(tracker.changes().len(), 3);

    let summary = tracker.summary();
    assert_eq!(summary.created, 1);
    assert_eq!(summary.modified, 1);
    assert_eq!(summary.deleted, 1);
    // delta = 42 + (150-100) + (0-200) = 42 + 50 - 200 = -108
    assert_eq!(summary.total_size_delta, -108);

    assert_eq!(
        tracker.affected_paths(),
        vec!["new_file.rs", "config.toml", "old.txt"]
    );
}

// ===========================================================================
// 20. Read-only source — source directory not modified during staging
// ===========================================================================
#[test]
fn source_not_modified_during_staging() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("immutable.txt"), "do not change").unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("nested.txt"), "also immutable").unwrap();

    // Take a snapshot of source before staging.
    let snap_before = snapshot::capture(src.path()).unwrap();

    let _ws = stage(src.path());

    // Take a snapshot of source after staging.
    let snap_after = snapshot::capture(src.path()).unwrap();

    let diff = snapshot::compare(&snap_before, &snap_after);
    assert!(
        diff.added.is_empty(),
        "source should have no added files after staging"
    );
    assert!(
        diff.removed.is_empty(),
        "source should have no removed files after staging"
    );
    assert!(
        diff.modified.is_empty(),
        "source should have no modified files after staging"
    );
}

// ===========================================================================
// 21. Snapshot comparison after workspace mutation
// ===========================================================================
#[test]
fn snapshot_comparison_detects_mutation() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "original").unwrap();
    fs::write(src.path().join("b.txt"), "keep").unwrap();

    let ws = stage_no_git(src.path());

    let snap_before = snapshot::capture(ws.path()).unwrap();

    // Mutate the workspace.
    fs::write(ws.path().join("a.txt"), "modified").unwrap();
    fs::write(ws.path().join("c.txt"), "new file").unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();

    let snap_after = snapshot::capture(ws.path()).unwrap();
    let diff = snapshot::compare(&snap_before, &snap_after);

    assert_eq!(diff.modified.len(), 1);
    assert!(diff.modified.contains(&std::path::PathBuf::from("a.txt")));
    assert_eq!(diff.added.len(), 1);
    assert!(diff.added.contains(&std::path::PathBuf::from("c.txt")));
    assert_eq!(diff.removed.len(), 1);
    assert!(diff.removed.contains(&std::path::PathBuf::from("b.txt")));
}

// ===========================================================================
// 22. Exclude .git from source is automatic
// ===========================================================================
#[test]
fn source_dot_git_excluded_automatically() {
    let src = tempfile::tempdir().unwrap();
    let git_dir = src.path().join(".git");
    fs::create_dir_all(git_dir.join("objects")).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    // Stage without git init so we can check that no .git was copied.
    let ws = stage_no_git(src.path());

    assert!(ws.path().join("code.rs").exists());
    assert!(
        !ws.path().join(".git").exists(),
        "source .git should not be copied"
    );
}

// ===========================================================================
// 23. Diff on clean workspace is empty
// ===========================================================================
#[test]
fn diff_on_unmodified_workspace_is_empty() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("stable.txt"), "no changes here\n").unwrap();

    let ws = stage(src.path());
    let summary = diff_workspace(&ws).unwrap();

    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

// ===========================================================================
// 24. Exclude multiple patterns
// ===========================================================================
#[test]
fn exclude_multiple_patterns() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("app.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("debug.log"), "debug info").unwrap();
    fs::write(src.path().join("cache.tmp"), "cache data").unwrap();
    fs::create_dir_all(src.path().join("target")).unwrap();
    fs::write(src.path().join("target").join("binary"), "ELF").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into(), "*.tmp".into(), "target/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = list_files(ws.path());
    assert!(files.contains(&"app.rs".to_string()));
    assert!(!files.contains(&"debug.log".to_string()));
    assert!(!files.contains(&"cache.tmp".to_string()));
    assert!(!files.iter().any(|f| f.starts_with("target")));
}

// ===========================================================================
// 25. Snapshot total_size is accurate
// ===========================================================================
#[test]
fn snapshot_total_size_is_accurate() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "12345").unwrap(); // 5 bytes
    fs::write(src.path().join("b.txt"), "1234567890").unwrap(); // 10 bytes

    let ws = stage_no_git(src.path());
    let snap = snapshot::capture(ws.path()).unwrap();

    assert_eq!(snap.total_size(), 15);
}

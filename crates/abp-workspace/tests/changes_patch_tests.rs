#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for changes, diff extraction, patch, and snapshot modules.

use abp_workspace::changes::{ChangeSet, FileChangeEntry, FileChangeKind, WorkspaceChangeTracker};
use abp_workspace::diff::{
    extract_file_diffs, extract_unified_diff, DiffAnalysis, DiffFilter, FileType,
};
use abp_workspace::patch::{
    apply_patch, create_patch, create_patch_with_header, validate_patch, Patch, PatchHeader,
};
use abp_workspace::snapshot::{
    capture, capture_with_contents, compare, compare_snapshots, restore_snapshot,
};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────────

/// Create a temp dir with a git repo and baseline commit.
fn setup_git_workspace() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path();

    // Create some initial files.
    fs::write(path.join("hello.txt"), "Hello, world!\n").unwrap();
    fs::write(path.join("readme.md"), "# README\n").unwrap();
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(path.join("src/main.rs"), "fn main() {}\n").unwrap();

    // Init git and make a baseline commit.
    run_git(path, &["init", "-q"]);
    run_git(path, &["add", "-A"]);
    run_git(
        path,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@local",
            "commit",
            "-qm",
            "baseline",
        ],
    );

    tmp
}

/// Create an empty git workspace.
fn setup_empty_git_workspace() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path();

    run_git(path, &["init", "-q"]);
    // Need at least one commit for diffs to work.
    fs::write(path.join(".gitkeep"), "").unwrap();
    run_git(path, &["add", "-A"]);
    run_git(
        path,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@local",
            "commit",
            "-qm",
            "baseline",
        ],
    );
    // Remove the gitkeep so workspace is truly empty (of user files).
    fs::remove_file(path.join(".gitkeep")).unwrap();

    tmp
}

fn run_git(path: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Change tracking tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn changeset_new_is_empty() {
    let cs = ChangeSet::new();
    assert!(cs.is_empty());
    assert_eq!(cs.len(), 0);
}

#[test]
fn changeset_summary_empty() {
    let cs = ChangeSet::new();
    assert_eq!(cs.change_summary(), "No changes detected.");
}

#[test]
fn changeset_filters_by_kind() {
    let mut cs = ChangeSet::new();
    cs.changes.push(FileChangeEntry {
        path: PathBuf::from("a.txt"),
        kind: FileChangeKind::Created,
    });
    cs.changes.push(FileChangeEntry {
        path: PathBuf::from("b.txt"),
        kind: FileChangeKind::Modified,
    });
    cs.changes.push(FileChangeEntry {
        path: PathBuf::from("c.txt"),
        kind: FileChangeKind::Deleted,
    });
    cs.changes.push(FileChangeEntry {
        path: PathBuf::from("d.txt"),
        kind: FileChangeKind::Renamed {
            old: "old_d.txt".to_string(),
            new: "d.txt".to_string(),
        },
    });

    assert_eq!(cs.created().len(), 1);
    assert_eq!(cs.modified().len(), 1);
    assert_eq!(cs.deleted().len(), 1);
    assert_eq!(cs.renamed().len(), 1);
    assert_eq!(cs.len(), 4);
}

#[test]
fn changeset_summary_includes_all_kinds() {
    let mut cs = ChangeSet::new();
    cs.total_additions = 10;
    cs.total_deletions = 3;
    cs.changes.push(FileChangeEntry {
        path: PathBuf::from("a.txt"),
        kind: FileChangeKind::Created,
    });
    cs.changes.push(FileChangeEntry {
        path: PathBuf::from("b.txt"),
        kind: FileChangeKind::Modified,
    });

    let summary = cs.change_summary();
    assert!(summary.contains("2 file(s) changed"));
    assert!(summary.contains("1 created"));
    assert!(summary.contains("1 modified"));
    assert!(summary.contains("+10"));
    assert!(summary.contains("-3"));
}

#[test]
fn file_change_kind_display() {
    assert_eq!(format!("{}", FileChangeKind::Created), "created");
    assert_eq!(format!("{}", FileChangeKind::Modified), "modified");
    assert_eq!(format!("{}", FileChangeKind::Deleted), "deleted");
    let renamed = FileChangeKind::Renamed {
        old: "a.txt".to_string(),
        new: "b.txt".to_string(),
    };
    assert!(format!("{renamed}").contains("a.txt"));
    assert!(format!("{renamed}").contains("b.txt"));
}

#[test]
fn change_tracker_detect_created_file() {
    let ws = setup_git_workspace();
    let path = ws.path();

    // Add a new file after baseline.
    fs::write(path.join("new_file.txt"), "new content\n").unwrap();

    let tracker = WorkspaceChangeTracker::new(path);
    assert!(tracker.has_changes());

    let changes = tracker.detect_changes().unwrap();
    assert!(!changes.is_empty());
    assert_eq!(changes.created().len(), 1);
    assert_eq!(changes.created()[0].path, PathBuf::from("new_file.txt"));
}

#[test]
fn change_tracker_detect_modified_file() {
    let ws = setup_git_workspace();
    let path = ws.path();

    // Modify existing file.
    fs::write(path.join("hello.txt"), "Modified content\n").unwrap();

    let tracker = WorkspaceChangeTracker::new(path);
    let changes = tracker.detect_changes().unwrap();
    assert_eq!(changes.modified().len(), 1);
}

#[test]
fn change_tracker_detect_deleted_file() {
    let ws = setup_git_workspace();
    let path = ws.path();

    // Delete existing file.
    fs::remove_file(path.join("hello.txt")).unwrap();

    let tracker = WorkspaceChangeTracker::new(path);
    let changes = tracker.detect_changes().unwrap();
    assert_eq!(changes.deleted().len(), 1);
    assert_eq!(changes.deleted()[0].path, PathBuf::from("hello.txt"));
}

#[test]
fn change_tracker_no_changes() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let tracker = WorkspaceChangeTracker::new(path);
    assert!(!tracker.has_changes());

    let changes = tracker.detect_changes().unwrap();
    assert!(changes.is_empty());
    assert_eq!(changes.change_summary(), "No changes detected.");
}

#[test]
fn change_tracker_detect_from_snapshots() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let before = capture(path).unwrap();

    // Make changes.
    fs::write(path.join("new_file.txt"), "hello\n").unwrap();
    fs::write(path.join("hello.txt"), "changed\n").unwrap();
    fs::remove_file(path.join("readme.md")).unwrap();

    let after = capture(path).unwrap();

    let changes = WorkspaceChangeTracker::detect_from_snapshots(&before, &after);
    assert!(!changes.is_empty());
    assert_eq!(changes.created().len(), 1);
    assert_eq!(changes.modified().len(), 1);
    assert_eq!(changes.deleted().len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Diff extraction tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extract_unified_diff_empty_workspace() {
    let ws = setup_git_workspace();
    let diff = extract_unified_diff(ws.path()).unwrap();
    // No changes => empty diff.
    assert!(diff.trim().is_empty());
}

#[test]
fn extract_unified_diff_with_changes() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("new.txt"), "line1\nline2\n").unwrap();

    let diff = extract_unified_diff(path).unwrap();
    assert!(diff.contains("new.txt"));
    assert!(diff.contains("+line1"));
}

#[test]
fn extract_file_diffs_returns_parsed_entries() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("added.rs"), "fn foo() {}\n").unwrap();
    fs::write(path.join("hello.txt"), "changed content\n").unwrap();

    let diffs = extract_file_diffs(path).unwrap();
    assert!(diffs.len() >= 2);

    let paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
    assert!(paths.contains(&"added.rs"));
    assert!(paths.contains(&"hello.txt"));
}

#[test]
fn extract_file_diffs_has_hunks() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("hello.txt"), "Changed!\n").unwrap();

    let diffs = extract_file_diffs(path).unwrap();
    let fd = diffs.iter().find(|d| d.path == "hello.txt").unwrap();
    assert!(!fd.hunks.is_empty());
    assert!(fd.additions > 0 || fd.deletions > 0);
}

#[test]
fn extract_file_diffs_stats_correct() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("new.txt"), "a\nb\nc\n").unwrap();

    let diffs = extract_file_diffs(path).unwrap();
    let fd = diffs.iter().find(|d| d.path == "new.txt").unwrap();
    assert_eq!(fd.additions, 3);
    assert_eq!(fd.deletions, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// DiffFilter tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_filter_by_file_type() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("code.rs"), "fn bar() {}\n").unwrap();
    fs::write(path.join("notes.md"), "# Notes\n").unwrap();
    fs::write(path.join("data.json"), "{}\n").unwrap();

    let diffs = extract_file_diffs(path).unwrap();

    let filter = DiffFilter::new().with_file_types(vec![FileType::Rust]);
    let filtered = filter.apply(&diffs);
    assert!(filtered.iter().all(|d| d.file_type == FileType::Rust));
    assert!(filtered.iter().any(|d| d.path == "code.rs"));
}

#[test]
fn diff_filter_exclude_patterns() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("keep.txt"), "keep\n").unwrap();
    fs::write(path.join("skip.log"), "log data\n").unwrap();

    let diffs = extract_file_diffs(path).unwrap();

    let filter = DiffFilter::new().with_exclude_patterns(vec!["*.log".to_string()]);
    let filtered = filter.apply(&diffs);
    assert!(filtered.iter().all(|d| !d.path.ends_with(".log")));
}

#[test]
fn diff_filter_permissive_by_default() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("a.txt"), "a\n").unwrap();
    fs::write(path.join("b.rs"), "fn b() {}\n").unwrap();

    let diffs = extract_file_diffs(path).unwrap();
    let filter = DiffFilter::new();
    let filtered = filter.apply(&diffs);
    assert_eq!(filtered.len(), diffs.len());
}

#[test]
fn diff_filter_apply_to_analysis() {
    let raw = concat!(
        "diff --git a/code.rs b/code.rs\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/code.rs\n",
        "@@ -0,0 +1,2 @@\n",
        "+fn foo() {}\n",
        "+fn bar() {}\n",
        "diff --git a/notes.md b/notes.md\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/notes.md\n",
        "@@ -0,0 +1 @@\n",
        "+# Notes\n",
    );
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 2);

    let filter = DiffFilter::new().with_file_types(vec![FileType::Rust]);
    let filtered = filter.apply_to_analysis(&analysis);
    assert_eq!(filtered.file_count(), 1);
    assert_eq!(filtered.total_additions, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Patch tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn create_patch_empty_workspace() {
    let ws = setup_git_workspace();
    let patch = create_patch(ws.path()).unwrap();
    assert!(patch.trim().is_empty());
}

#[test]
fn create_patch_captures_changes() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("new_file.txt"), "patch content\n").unwrap();

    let patch = create_patch(path).unwrap();
    assert!(patch.contains("new_file.txt"));
    assert!(patch.contains("+patch content"));
}

#[test]
fn validate_patch_empty_is_valid() {
    let ws = setup_git_workspace();
    assert!(validate_patch(ws.path(), "").unwrap());
}

#[test]
fn validate_patch_valid_patch() {
    let ws = setup_git_workspace();
    let path = ws.path();

    // Make a change and create a patch.
    fs::write(path.join("hello.txt"), "Modified for patch\n").unwrap();
    let patch = create_patch(path).unwrap();

    // Revert the change.
    fs::write(path.join("hello.txt"), "Hello, world!\n").unwrap();

    // The patch should still validate cleanly.
    assert!(validate_patch(path, &patch).unwrap());
}

#[test]
fn apply_patch_empty_is_noop() {
    let ws = setup_git_workspace();
    apply_patch(ws.path(), "").unwrap();
}

#[test]
fn patch_round_trip() {
    let ws = setup_git_workspace();
    let path = ws.path();

    // Make changes.
    fs::write(path.join("hello.txt"), "Patched content\n").unwrap();
    fs::write(path.join("extra.txt"), "Extra file\n").unwrap();

    // Create patch.
    let patch = create_patch(path).unwrap();
    assert!(!patch.trim().is_empty());

    // Revert changes.
    fs::write(path.join("hello.txt"), "Hello, world!\n").unwrap();
    fs::remove_file(path.join("extra.txt")).unwrap();

    // Apply patch — should restore the changes.
    apply_patch(path, &patch).unwrap();

    let content = fs::read_to_string(path.join("hello.txt")).unwrap();
    assert!(
        content.contains("Patched content"),
        "expected patched content, got: {content:?}"
    );
    assert!(path.join("extra.txt").exists());
    let extra = fs::read_to_string(path.join("extra.txt")).unwrap();
    assert!(
        extra.contains("Extra file"),
        "expected extra content, got: {extra:?}"
    );
}

#[test]
fn apply_patch_invalid_fails() {
    let ws = setup_git_workspace();
    let bad_patch = concat!(
        "diff --git a/nonexistent.txt b/nonexistent.txt\n",
        "--- a/nonexistent.txt\n",
        "+++ b/nonexistent.txt\n",
        "@@ -1,3 +1,3 @@\n",
        "-old line that does not exist\n",
        "+new line\n",
    );
    let result = apply_patch(ws.path(), bad_patch);
    assert!(result.is_err());
}

#[test]
fn create_patch_with_header_metadata() {
    let ws = setup_git_workspace();
    let path = ws.path();

    fs::write(path.join("new.txt"), "content\n").unwrap();

    let patch = create_patch_with_header(path, "test patch").unwrap();
    assert_eq!(patch.header.description, "test patch");
    assert_eq!(patch.header.author, "abp");
    assert_eq!(patch.header.from, "HEAD");
    assert_eq!(patch.header.to, "working-tree");
    assert!(!patch.is_empty());
}

#[test]
fn patch_to_patch_string_format() {
    let patch = Patch {
        header: PatchHeader {
            from: "abc123".to_string(),
            to: "def456".to_string(),
            author: "test-author".to_string(),
            date: chrono::Utc::now(),
            description: "my patch".to_string(),
        },
        diff_content: "+added line\n".to_string(),
    };

    let s = patch.to_patch_string();
    assert!(s.contains("From: test-author"));
    assert!(s.contains("Subject: my patch"));
    assert!(s.contains("+added line"));
    assert!(s.contains("---"));
}

#[test]
fn patch_is_empty_for_no_diff() {
    let patch = Patch {
        header: PatchHeader {
            from: "a".to_string(),
            to: "b".to_string(),
            author: "x".to_string(),
            date: chrono::Utc::now(),
            description: "empty".to_string(),
        },
        diff_content: String::new(),
    };
    assert!(patch.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Snapshot tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn compare_snapshots_alias_matches_compare() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let before = capture(path).unwrap();
    fs::write(path.join("new.txt"), "hello\n").unwrap();
    let after = capture(path).unwrap();

    let diff1 = compare(&before, &after);
    let diff2 = compare_snapshots(&before, &after);

    assert_eq!(diff1.added, diff2.added);
    assert_eq!(diff1.removed, diff2.removed);
    assert_eq!(diff1.modified, diff2.modified);
    assert_eq!(diff1.unchanged, diff2.unchanged);
}

#[test]
fn snapshot_captures_all_files() {
    let ws = setup_git_workspace();
    let snap = capture(ws.path()).unwrap();
    // baseline has: hello.txt, readme.md, src/main.rs
    assert_eq!(snap.file_count(), 3);
    assert!(snap.has_file(Path::new("hello.txt")));
    assert!(snap.has_file(Path::new("readme.md")));
}

#[test]
fn snapshot_compare_detects_added() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let before = capture(path).unwrap();
    fs::write(path.join("added.txt"), "new file\n").unwrap();
    let after = capture(path).unwrap();

    let diff = compare(&before, &after);
    assert_eq!(diff.added.len(), 1);
    assert!(diff.removed.is_empty());
}

#[test]
fn snapshot_compare_detects_removed() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let before = capture(path).unwrap();
    fs::remove_file(path.join("hello.txt")).unwrap();
    let after = capture(path).unwrap();

    let diff = compare(&before, &after);
    assert_eq!(diff.removed.len(), 1);
}

#[test]
fn snapshot_compare_detects_modified() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let before = capture(path).unwrap();
    fs::write(path.join("hello.txt"), "modified\n").unwrap();
    let after = capture(path).unwrap();

    let diff = compare(&before, &after);
    assert_eq!(diff.modified.len(), 1);
}

#[test]
fn restore_snapshot_removes_extra_files() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let (snap, contents) = capture_with_contents(path).unwrap();

    // Add extra file that wasn't in the snapshot.
    fs::write(path.join("extra.txt"), "extra\n").unwrap();
    assert!(path.join("extra.txt").exists());

    restore_snapshot(path, &snap, Some(&contents)).unwrap();

    // Extra file should be gone.
    assert!(!path.join("extra.txt").exists());
}

#[test]
fn restore_snapshot_restores_deleted_files() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let (snap, contents) = capture_with_contents(path).unwrap();

    // Delete a file.
    fs::remove_file(path.join("hello.txt")).unwrap();
    assert!(!path.join("hello.txt").exists());

    restore_snapshot(path, &snap, Some(&contents)).unwrap();

    // File should be back.
    assert!(path.join("hello.txt").exists());
    let content = fs::read_to_string(path.join("hello.txt")).unwrap();
    assert_eq!(content, "Hello, world!\n");
}

#[test]
fn restore_snapshot_restores_modified_files() {
    let ws = setup_git_workspace();
    let path = ws.path();

    let (snap, contents) = capture_with_contents(path).unwrap();

    // Modify a file.
    fs::write(path.join("hello.txt"), "changed\n").unwrap();

    restore_snapshot(path, &snap, Some(&contents)).unwrap();

    let content = fs::read_to_string(path.join("hello.txt")).unwrap();
    assert_eq!(content, "Hello, world!\n");
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_workspace_diff() {
    let ws = setup_empty_git_workspace();
    let diff = extract_unified_diff(ws.path()).unwrap();
    // Removing .gitkeep will show as a deletion, or it might be empty.
    // The workspace has no files so any diff is about the gitkeep removal.
    let _ = diff; // Just verify it doesn't error.
}

#[test]
fn empty_workspace_snapshot() {
    let ws = setup_empty_git_workspace();
    let snap = capture(ws.path()).unwrap();
    // Only .gitkeep was removed, so 0 files.
    assert_eq!(snap.file_count(), 0);
}

#[test]
fn binary_file_in_diff() {
    let ws = setup_git_workspace();
    let path = ws.path();

    // Write a binary file (contains null bytes).
    fs::write(path.join("binary.bin"), b"\x00\x01\x02\x03").unwrap();

    let diffs = extract_file_diffs(path).unwrap();
    let bin = diffs.iter().find(|d| d.path.contains("binary"));
    // Git may report it as binary or as text depending on content detection.
    // Either way it should not error.
    assert!(bin.is_some() || !diffs.is_empty());
}

#[test]
fn changeset_serde_round_trip() {
    let mut cs = ChangeSet::new();
    cs.total_additions = 5;
    cs.total_deletions = 2;
    cs.changes.push(FileChangeEntry {
        path: PathBuf::from("test.rs"),
        kind: FileChangeKind::Created,
    });
    cs.changes.push(FileChangeEntry {
        path: PathBuf::from("old.txt"),
        kind: FileChangeKind::Renamed {
            old: "old.txt".to_string(),
            new: "new.txt".to_string(),
        },
    });

    let json = serde_json::to_string(&cs).unwrap();
    let back: ChangeSet = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(back.total_additions, 5);
}

#[test]
fn patch_header_serde_round_trip() {
    let header = PatchHeader {
        from: "abc".to_string(),
        to: "def".to_string(),
        author: "test".to_string(),
        date: chrono::Utc::now(),
        description: "test desc".to_string(),
    };
    let json = serde_json::to_string(&header).unwrap();
    let back: PatchHeader = serde_json::from_str(&json).unwrap();
    assert_eq!(header.from, back.from);
    assert_eq!(header.description, back.description);
}

#[test]
fn multiple_file_changes_tracked() {
    let ws = setup_git_workspace();
    let path = ws.path();

    // Create, modify, and delete files.
    fs::write(path.join("new1.txt"), "new1\n").unwrap();
    fs::write(path.join("new2.txt"), "new2\n").unwrap();
    fs::write(path.join("hello.txt"), "modified hello\n").unwrap();
    fs::remove_file(path.join("readme.md")).unwrap();

    let tracker = WorkspaceChangeTracker::new(path);
    let changes = tracker.detect_changes().unwrap();

    let summary = changes.change_summary();
    assert!(summary.contains("file(s) changed"));
    assert!(changes.created().len() >= 2);
    assert!(changes.modified().len() >= 1);
    assert!(changes.deleted().len() >= 1);
}

#[test]
fn diff_filter_combined_type_and_pattern() {
    let raw = concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/src/lib.rs\n",
        "@@ -0,0 +1 @@\n",
        "+pub mod lib;\n",
        "diff --git a/src/test.rs b/src/test.rs\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/src/test.rs\n",
        "@@ -0,0 +1 @@\n",
        "+#[test] fn t() {}\n",
        "diff --git a/readme.md b/readme.md\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/readme.md\n",
        "@@ -0,0 +1 @@\n",
        "+# Hello\n",
    );
    let analysis = DiffAnalysis::parse(raw);

    // Filter to Rust files only, then also exclude test files.
    let filter = DiffFilter::new()
        .with_file_types(vec![FileType::Rust])
        .with_exclude_patterns(vec!["**/test*".to_string()]);

    let filtered = filter.apply_to_analysis(&analysis);
    assert_eq!(filtered.file_count(), 1);
    assert_eq!(filtered.files[0].path, "src/lib.rs");
}

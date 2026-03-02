// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for `abp_workspace::diff` module.

use abp_workspace::WorkspaceStager;
use abp_workspace::diff::{DiffSummary, diff_workspace};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a staged workspace with git init from the given source directory.
fn staged(src: &std::path::Path) -> abp_workspace::PreparedWorkspace {
    WorkspaceStager::new()
        .source_root(src)
        .stage()
        .expect("staging should succeed")
}

// ---------------------------------------------------------------------------
// 1. No changes → empty diff
// ---------------------------------------------------------------------------
#[test]
fn no_changes_yields_empty_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let ws = staged(src.path());
    let summary = diff_workspace(&ws).unwrap();

    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
    assert_eq!(summary.total_additions, 0);
    assert_eq!(summary.total_deletions, 0);
}

// ---------------------------------------------------------------------------
// 2. Add files → diff shows additions
// ---------------------------------------------------------------------------
#[test]
fn added_files_detected() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.txt"), "seed").unwrap();

    let ws = staged(src.path());
    fs::write(ws.path().join("new_a.txt"), "alpha\nbeta\n").unwrap();
    fs::write(ws.path().join("new_b.txt"), "gamma\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();

    assert!(!summary.is_empty());
    assert!(summary.added.contains(&PathBuf::from("new_a.txt")));
    assert!(summary.added.contains(&PathBuf::from("new_b.txt")));
    assert!(summary.modified.is_empty());
    assert!(summary.deleted.is_empty());
    assert_eq!(summary.total_additions, 3); // 2 + 1
    assert_eq!(summary.total_deletions, 0);
}

// ---------------------------------------------------------------------------
// 3. Modify files → diff shows modifications
// ---------------------------------------------------------------------------
#[test]
fn modified_files_detected() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("readme.md"), "original\n").unwrap();

    let ws = staged(src.path());
    fs::write(ws.path().join("readme.md"), "changed\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();

    assert!(!summary.is_empty());
    assert!(summary.modified.contains(&PathBuf::from("readme.md")));
    assert!(summary.added.is_empty());
    assert!(summary.deleted.is_empty());
    assert!(summary.total_additions >= 1);
    assert!(summary.total_deletions >= 1);
}

// ---------------------------------------------------------------------------
// 4. Delete files → diff shows deletions
// ---------------------------------------------------------------------------
#[test]
fn deleted_files_detected() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "will die\nbye\n").unwrap();

    let ws = staged(src.path());
    fs::remove_file(ws.path().join("doomed.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();

    assert!(!summary.is_empty());
    assert!(summary.deleted.contains(&PathBuf::from("doomed.txt")));
    assert!(summary.added.is_empty());
    assert!(summary.modified.is_empty());
    assert_eq!(summary.total_additions, 0);
    assert!(summary.total_deletions >= 2);
}

// ---------------------------------------------------------------------------
// 5. Mixed changes → all categories populated
// ---------------------------------------------------------------------------
#[test]
fn mixed_changes_all_categories() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("modify.txt"), "old\n").unwrap();
    fs::write(src.path().join("delete.txt"), "remove me\n").unwrap();
    fs::write(src.path().join("keep.txt"), "unchanged\n").unwrap();

    let ws = staged(src.path());

    fs::write(ws.path().join("modify.txt"), "new\n").unwrap();
    fs::remove_file(ws.path().join("delete.txt")).unwrap();
    fs::write(ws.path().join("added.txt"), "fresh\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();

    assert_eq!(summary.added, vec![PathBuf::from("added.txt")]);
    assert_eq!(summary.modified, vec![PathBuf::from("modify.txt")]);
    assert_eq!(summary.deleted, vec![PathBuf::from("delete.txt")]);
    assert_eq!(summary.file_count(), 3);
    assert!(summary.total_additions > 0);
    assert!(summary.total_deletions > 0);
}

// ---------------------------------------------------------------------------
// 6. Large diff → handles many files
// ---------------------------------------------------------------------------
#[test]
fn large_diff_many_files() {
    let src = tempdir().unwrap();
    for i in 0..50 {
        fs::write(
            src.path().join(format!("file_{i}.txt")),
            format!("line {i}\n"),
        )
        .unwrap();
    }

    let ws = staged(src.path());

    // Modify 20, delete 10, add 15
    for i in 0..20 {
        fs::write(
            ws.path().join(format!("file_{i}.txt")),
            format!("changed {i}\n"),
        )
        .unwrap();
    }
    for i in 20..30 {
        fs::remove_file(ws.path().join(format!("file_{i}.txt"))).unwrap();
    }
    for i in 50..65 {
        fs::write(
            ws.path().join(format!("file_{i}.txt")),
            format!("new {i}\n"),
        )
        .unwrap();
    }

    let summary = diff_workspace(&ws).unwrap();

    assert_eq!(summary.modified.len(), 20);
    assert_eq!(summary.deleted.len(), 10);
    assert_eq!(summary.added.len(), 15);
    assert_eq!(summary.file_count(), 45);
}

// ---------------------------------------------------------------------------
// 7. Binary file changes → counted but no line counts
// ---------------------------------------------------------------------------
#[test]
fn binary_files_counted_no_line_counts() {
    let src = tempdir().unwrap();
    let png_header: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    ];
    fs::write(src.path().join("image.png"), &png_header).unwrap();

    let ws = staged(src.path());

    // Modify the binary file
    let mut modified = png_header;
    modified.extend_from_slice(&[0xFF; 32]);
    fs::write(ws.path().join("image.png"), &modified).unwrap();

    // Add a new binary file
    fs::write(ws.path().join("data.bin"), vec![0u8; 64]).unwrap();

    let summary = diff_workspace(&ws).unwrap();

    assert!(summary.modified.contains(&PathBuf::from("image.png")));
    assert!(summary.added.contains(&PathBuf::from("data.bin")));
    // Binary files should not contribute to line counts.
    assert_eq!(summary.total_additions, 0);
    assert_eq!(summary.total_deletions, 0);
    assert_eq!(summary.file_count(), 2);
}

// ---------------------------------------------------------------------------
// 8. Serde roundtrip
// ---------------------------------------------------------------------------
#[test]
fn serde_roundtrip() {
    let summary = DiffSummary {
        added: vec![PathBuf::from("new.txt")],
        modified: vec![PathBuf::from("changed.rs")],
        deleted: vec![PathBuf::from("old.log")],
        total_additions: 42,
        total_deletions: 7,
    };

    let json = serde_json::to_string(&summary).unwrap();
    let deserialized: DiffSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(summary, deserialized);
}

// ---------------------------------------------------------------------------
// 9. DiffSummary::is_empty on default
// ---------------------------------------------------------------------------
#[test]
fn default_summary_is_empty() {
    let summary = DiffSummary::default();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
    assert_eq!(summary.total_changes(), 0);
}

// ---------------------------------------------------------------------------
// 10. DiffSummary methods with values
// ---------------------------------------------------------------------------
#[test]
fn summary_methods_with_values() {
    let summary = DiffSummary {
        added: vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")],
        modified: vec![PathBuf::from("c.txt")],
        deleted: vec![
            PathBuf::from("d.txt"),
            PathBuf::from("e.txt"),
            PathBuf::from("f.txt"),
        ],
        total_additions: 100,
        total_deletions: 50,
    };

    assert!(!summary.is_empty());
    assert_eq!(summary.file_count(), 6);
    assert_eq!(summary.total_changes(), 150);
}

// ---------------------------------------------------------------------------
// 11. Nested directory additions
// ---------------------------------------------------------------------------
#[test]
fn nested_directory_additions() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "root\n").unwrap();

    let ws = staged(src.path());

    let nested = ws.path().join("sub").join("deep");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("nested.txt"), "deep content\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();

    assert!(
        summary
            .added
            .contains(&PathBuf::from("sub/deep/nested.txt"))
    );
    assert_eq!(summary.total_additions, 1);
}

// ---------------------------------------------------------------------------
// 12. Empty workspace (no initial files) stays empty
// ---------------------------------------------------------------------------
#[test]
fn empty_workspace_stays_empty() {
    let src = tempdir().unwrap();
    // No files at all

    let ws = staged(src.path());
    let summary = diff_workspace(&ws).unwrap();

    assert!(summary.is_empty());
}

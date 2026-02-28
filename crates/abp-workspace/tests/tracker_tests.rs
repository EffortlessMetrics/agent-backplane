// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the `tracker` module.

use abp_workspace::tracker::{ChangeKind, ChangeSummary, ChangeTracker, FileChange};

fn created(path: &str, size: u64) -> FileChange {
    FileChange {
        path: path.to_string(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(size),
        content_hash: Some("abc123".to_string()),
    }
}

fn modified(path: &str, before: u64, after: u64) -> FileChange {
    FileChange {
        path: path.to_string(),
        kind: ChangeKind::Modified,
        size_before: Some(before),
        size_after: Some(after),
        content_hash: Some("def456".to_string()),
    }
}

fn deleted(path: &str, size: u64) -> FileChange {
    FileChange {
        path: path.to_string(),
        kind: ChangeKind::Deleted,
        size_before: Some(size),
        size_after: None,
        content_hash: None,
    }
}

fn renamed(path: &str, from: &str, size: u64) -> FileChange {
    FileChange {
        path: path.to_string(),
        kind: ChangeKind::Renamed {
            from: from.to_string(),
        },
        size_before: Some(size),
        size_after: Some(size),
        content_hash: Some("ren789".to_string()),
    }
}

#[test]
fn new_tracker_is_empty() {
    let t = ChangeTracker::new();
    assert!(!t.has_changes());
    assert!(t.changes().is_empty());
}

#[test]
fn record_single_change() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 100));
    assert!(t.has_changes());
    assert_eq!(t.changes().len(), 1);
    assert_eq!(t.changes()[0].path, "a.txt");
}

#[test]
fn summary_counts_created() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 10));
    t.record(created("b.txt", 20));
    let s = t.summary();
    assert_eq!(s.created, 2);
    assert_eq!(s.modified, 0);
    assert_eq!(s.deleted, 0);
    assert_eq!(s.renamed, 0);
}

#[test]
fn summary_counts_all_kinds() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 10));
    t.record(modified("b.txt", 20, 30));
    t.record(deleted("c.txt", 40));
    t.record(renamed("d.txt", "old_d.txt", 50));
    let s = t.summary();
    assert_eq!(s.created, 1);
    assert_eq!(s.modified, 1);
    assert_eq!(s.deleted, 1);
    assert_eq!(s.renamed, 1);
}

#[test]
fn summary_size_delta_positive() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 100));
    assert_eq!(t.summary().total_size_delta, 100);
}

#[test]
fn summary_size_delta_negative() {
    let mut t = ChangeTracker::new();
    t.record(deleted("a.txt", 200));
    assert_eq!(t.summary().total_size_delta, -200);
}

#[test]
fn summary_size_delta_mixed() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 50));
    t.record(deleted("b.txt", 100));
    t.record(modified("c.txt", 30, 80));
    // delta = 50 + (-100) + (80-30) = 0
    assert_eq!(t.summary().total_size_delta, 0);
}

#[test]
fn by_kind_filters_correctly() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 10));
    t.record(modified("b.txt", 20, 30));
    t.record(created("c.txt", 40));
    let created_changes = t.by_kind(&ChangeKind::Created);
    assert_eq!(created_changes.len(), 2);
    assert_eq!(created_changes[0].path, "a.txt");
    assert_eq!(created_changes[1].path, "c.txt");
}

#[test]
fn by_kind_renamed_matches_variant() {
    let mut t = ChangeTracker::new();
    t.record(renamed("new.txt", "old.txt", 10));
    t.record(renamed("new2.txt", "old2.txt", 20));
    let r = t.by_kind(&ChangeKind::Renamed {
        from: "old.txt".to_string(),
    });
    // PartialEq on the full variant, so only exact match
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "new.txt");
}

#[test]
fn by_kind_empty_result() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 10));
    assert!(t.by_kind(&ChangeKind::Deleted).is_empty());
}

#[test]
fn affected_paths_preserves_order() {
    let mut t = ChangeTracker::new();
    t.record(created("b.txt", 1));
    t.record(modified("a.txt", 1, 2));
    t.record(deleted("c.txt", 1));
    assert_eq!(t.affected_paths(), vec!["b.txt", "a.txt", "c.txt"]);
}

#[test]
fn affected_paths_deduplicates() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 10));
    t.record(modified("a.txt", 10, 20));
    assert_eq!(t.affected_paths(), vec!["a.txt"]);
}

#[test]
fn clear_resets_tracker() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 10));
    t.record(deleted("b.txt", 20));
    assert!(t.has_changes());
    t.clear();
    assert!(!t.has_changes());
    assert!(t.changes().is_empty());
    let s = t.summary();
    assert_eq!(s, ChangeSummary::default());
}

#[test]
fn default_summary_is_zero() {
    let s = ChangeSummary::default();
    assert_eq!(s.created, 0);
    assert_eq!(s.modified, 0);
    assert_eq!(s.deleted, 0);
    assert_eq!(s.renamed, 0);
    assert_eq!(s.total_size_delta, 0);
}

#[test]
fn serde_round_trip_file_change() {
    let fc = created("test.rs", 42);
    let json = serde_json::to_string(&fc).unwrap();
    let back: FileChange = serde_json::from_str(&json).unwrap();
    assert_eq!(fc, back);
}

#[test]
fn serde_round_trip_change_summary() {
    let mut t = ChangeTracker::new();
    t.record(created("a.txt", 10));
    t.record(deleted("b.txt", 30));
    let s = t.summary();
    let json = serde_json::to_string(&s).unwrap();
    let back: ChangeSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
}

#[test]
fn serde_renamed_variant_includes_from() {
    let fc = renamed("new.txt", "old.txt", 100);
    let json = serde_json::to_string(&fc).unwrap();
    assert!(json.contains("old.txt"));
    let back: FileChange = serde_json::from_str(&json).unwrap();
    assert_eq!(fc, back);
}
